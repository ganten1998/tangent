#!/usr/bin/env python3
"""Score a denoised file against the dry source and against a reference tool.

    scripts/denoise-score.py CANDIDATE.wav [MORE.wav ...]

Reads ~/Desktop/challenge/{dry,desirable,undesirable}.wav as the fixed
comparison set: one dry take, the same take through a tool whose result the
owner called "tight yet open non digital sounding", and the same take through
ours, which he called "undesirable" and then described exactly -- "a crispy
bathroom sounding frequency that sounds like a digital artifact".

That sentence is a measurement, and these are the two numbers it turns into.

BATHROOM HUMP is the residual's tilt between words: the mid band against the
mean of the low and the air. A denoiser that lowers the floor without
recolouring it leaves that near zero. Ours leaves +2.9 dB -- it takes 11 dB out
of the bottom and the top octaves and only 8 out of 800 Hz-5 kHz, so what is
left is mid-forward and boxy, which is what a small tiled room sounds like.

RESIDUAL FLATNESS is the same fault from the other side. The reference leaves a
residual FLATTER than the noise it removed (0.270 against the source's 0.268);
ours leaves one markedly peakier (0.183). A flat quiet floor reads as air. A
peaky one reads as an artifact however quiet it is, which is why matching the
reference's total reduction -- both are about 10 dB, and about 7 dB once makeup
gain is taken out -- does not get us there.

Averaging is per-FRAME dB and never dB-of-summed-energy: a noise measurement
that sums energy across frames is decided by its loudest frame, which is the
one frame that is not noise.

Generate a candidate with the probe on the other side of this loop:

    target/release/tangent --denoise-test IN.wav,OUT.wav[,reduce=DB][,kernel=K]
"""

import numpy as np, sys, os, struct

def read_wav(path):
    b=open(path,'rb').read()
    assert b[:4]==b'RIFF' and b[8:12]==b'WAVE', f"{path} is not a WAV"
    at=12; fmt=None; data=None
    while at+8<=len(b):
        cid=b[at:at+4]; sz=struct.unpack('<I',b[at+4:at+8])[0]; body=b[at+8:at+8+sz]
        if cid==b'fmt ': fmt=body
        elif cid==b'data': data=body
        at+=8+sz+(sz&1)
    tag,ch,sr,_,_,bits=struct.unpack('<HHIIHH',fmt[:16])
    if tag==0xFFFE: tag=struct.unpack('<H',fmt[24:26])[0]
    if tag==3 and bits==32: v=np.frombuffer(data,dtype='<f4').astype(np.float64)
    elif tag==1 and bits==16: v=np.frombuffer(data,dtype='<i2').astype(np.float64)/32768.0
    elif tag==1 and bits==24:
        a=np.frombuffer(data,dtype=np.uint8).reshape(-1,3).astype(np.int32)
        x=a[:,0]|(a[:,1]<<8)|(a[:,2]<<16)
        v=np.where(x&0x800000,x-0x1000000,x).astype(np.float64)/8388608.0
    elif tag==1 and bits==32: v=np.frombuffer(data,dtype='<i4').astype(np.float64)/2147483648.0
    else: raise SystemExit(f"{path}: tag {tag} bits {bits} not handled")
    return v.reshape(-1,ch).mean(axis=1), sr

def align(a,b,span=4000):
    n=min(len(a),len(b)); a=a[:n].copy(); b=b[:n].copy()
    seg=slice(int(0.15*n),int(0.55*n))
    x=a[seg]-a[seg].mean(); y=b[seg]-b[seg].mean()
    c=np.correlate(y,x[span:-span],mode='valid')
    lag=int(np.argmax(np.abs(c))-span)
    if lag>0: b=np.concatenate([b[lag:],np.zeros(lag)])
    elif lag<0: b=np.concatenate([np.zeros(-lag),b[:lag]])
    return b,lag

WIN=1024; HOP=256
BANDS={'low 100-400':(100,400),'lomid 400-800':(400,800),'mid 800-5k':(800,5000),
       'hi 5k-10k':(5000,10000),'air 10k-18k':(10000,18000)}

def frames(v,n):
    w=np.hanning(WIN); nf=(n-WIN)//HOP
    return np.abs(np.array([np.fft.rfft(v[i*HOP:i*HOP+WIN]*w) for i in range(nf)]))**2+1e-22

def masks(Pd):
    lvl=10*np.log10(Pd.sum(1))
    live=lvl>-100                        # not digital silence
    q=np.percentile(lvl[live],30)        # the quiet 30% of real frames = room tone
    v=np.percentile(lvl[live],85)
    return live&(lvl<q), live&(lvl>v)

def analyse(dry,out,sr,name,ref=None):
    n=min(len(dry),len(out)); dry=dry[:n]; out=out[:n]
    Pd=frames(dry,n); Po=frames(out,n)
    f=np.fft.rfftfreq(WIN,1/sr)
    noise,voice=masks(Pd)
    def g(band,m):     # mean of per-frame dB
        return float(np.mean(10*np.log10(Po[m][:,band].sum(1)/Pd[m][:,band].sum(1))))
    res={k:g((f>=lo)&(f<hi),noise) for k,(lo,hi) in BANDS.items()}
    voi={k:g((f>=lo)&(f<hi),voice) for k,(lo,hi) in BANDS.items()}
    hump=res['mid 800-5k']-0.5*(res['low 100-400']+res['air 10k-18k'])
    b=(f>=200)&(f<16000)
    fl=lambda P: float(np.exp(np.mean(np.log(P[noise][:,b].mean(0))))/np.mean(P[noise][:,b].mean(0)))
    # how much of the reduction is just makeup gain
    makeup=float(np.mean(list(voi.values())))
    nr=float(np.mean(list(res.values())))
    print(f"\n--- {name} ---")
    print("  between words : "+"  ".join(f"{k.split()[0]:>5} {v:6.2f}" for k,v in res.items()))
    print("  during voice  : "+"  ".join(f"{k.split()[0]:>5} {v:6.2f}" for k,v in voi.items()))
    print(f"  noise down {nr:6.2f} dB   voice down {makeup:6.2f} dB   TRUE reduction {nr-makeup:6.2f} dB")
    print(f"  BATHROOM HUMP  {hump:+6.2f} dB   (mid vs low+air; want <= 0, reference is the target)")
    print(f"  residual flatness {fl(Po):.4f}   dry noise is {fl(Pd):.4f}   (want >= dry: flatter than what it removed)")
    return dict(res=res,voi=voi,hump=hump,flat=fl(Po),dryflat=fl(Pd),nr=nr,makeup=makeup)

if __name__=="__main__":
    ch=os.path.expanduser("~/Desktop/challenge")
    dry,sr=read_wav(f"{ch}/dry.wav")
    tg=[("REFERENCE  desirable.wav",f"{ch}/desirable.wav"),
        ("SHIPPED    undesirable.wav",f"{ch}/undesirable.wav")]
    for p in sys.argv[1:]: tg.append((f"CANDIDATE  {os.path.basename(p)}",p))
    for name,path in tg:
        o,_=read_wav(path); o,lag=align(dry,o)
        analyse(dry,o,sr,f"{name}  [aligned {lag:+d} samples]")
