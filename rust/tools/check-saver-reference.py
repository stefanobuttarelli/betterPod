#!/usr/bin/env python3
"""Compare original Go visualizer equations with the Rust port, locally.

Extracts unchanged reference function bodies from the checked-out Go source.
The input is fixed rational PCM, not a microphone/audio-device recording.
"""
from pathlib import Path
import json, os, re, subprocess
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'build/rust/saver-reference'
OUT.mkdir(parents=True,exist_ok=True)
visual=(ROOT/'src/ij_visualizer.go').read_text()
colors=(ROOT/'src/ij_vizcolors.go').read_text()
def function(source,name):
    match=re.search(r'^func (?:\([^\n]+\) )?'+re.escape(name)+r'\(',source,re.M)
    if not match:raise RuntimeError(name)
    start=source.index('{',match.start());depth=1;i=start+1
    while depth:
        if source[i]=='{':depth+=1
        if source[i]=='}':depth-=1
        i+=1
    return source[match.start():i]
types=visual[visual.index('type vizParticle struct'):visual.index('func newVisualizer()')]
schemes=colors[colors.index('type vizScheme struct'):colors.index('// saverColors:')]
parts=['''package main
import("image";"image/color";"image/draw";"math";"math/cmplx";"math/rand";"time";"os";"fmt")
const(vizN=1024;vizBands=40;vizW=320;vizH=240;vizTop=0;vizBarW=11;vizBarGap=4;vizBarMax=280;vizBase=330;vizReflH=150;vizSpecStep=2)
type Col struct{R,G,B,A float64};type Album struct{}
func lerpCol(a,b Col,t float64)Col{return Col{a.R+(b.R-a.R)*t,a.G+(b.G-a.G)*t,a.B+(b.B-a.B)*t,a.A+(b.A-a.A)*t}}
func parallelRows(a,b int,f func(int,int)){f(a,b)}
func abs(n int)int{if n<0{return -n};return n}
func blitOpaque(dst,src *image.RGBA,x,y int){draw.Draw(dst,image.Rect(x,y,x+src.Bounds().Dx(),y+src.Bounds().Dy()),src,src.Bounds().Min,draw.Src)}
func fillRect(dst *image.RGBA,x,y,w,h int,c Col){draw.Draw(dst,image.Rect(x,y,x+w,y+h),&image.Uniform{color.RGBA{uint8(c.R),uint8(c.G),uint8(c.B),255}},image.Point{},draw.Src)}
var vizNames=[]string{"Bars","Waves","Tunnel","Stars","Spectrogram"};var saverEffect=0
''',types,schemes,'var vizMagma=vizSchemes[2].stops\n']
for name in ['newVisualizer','setRate','analyse','energy','makeBars','drawBars','vizFade','vizRemap','add','sat','color','line','makeMaps','drawWaves','drawTunnel','drawStars','hsvRGB']:
    body=function(visual,name)
    if name=='newVisualizer':body=body.replace('int(C.audio_vis_rate())','44100')
    if name=='analyse':
        body=re.sub(r'\tif r := int\(C.audio_vis_rate\(\)\); r != v.rate \{\n\t\tv.setRate\(r\)\n\t\}', '',body)
        body=re.sub(r'v.fresh = C.audio_vis_read\([^\n]+','v.fresh = true',body)
    parts.append(body+'\n')
for name in ['schemeAt','applyColors','specRows','drawSpectro']:parts.append(function(colors,name)+'\n')
parts.append('''func main(){out:=os.Args[1];os.MkdirAll(out,0755)
 for effect:=0;effect<5;effect++{v:=newVisualizer();v.rng=rand.New(rand.NewSource(1));v.mapAt=time.Unix(100,0);if effect==3{v.parts=[]vizParticle{{160,120,1.3,-0.9,1,.15},{160,120,-.8,1.7,1,.7}}}
 for frame:=0;frame<30;frame++{for i:=range v.samples{v.samples[i]=float32((i*37+frame*17)%257)/512-.25};now:=time.Unix(100,0).Add(time.Duration(frame)*time.Second/30);v.analyse(now);switch effect{case 1:v.drawWaves();case 2:v.drawTunnel(now);case 3:v.fresh=false;v.beat=false;v.drawStars();case 4:v.drawSpectro()}}
 pixels:=v.buf;if effect==0{dst:=image.NewRGBA(image.Rect(0,0,640,480));v.drawBars(dst);pixels=dst.Pix};for i:=3;i<len(pixels);i+=4{pixels[i]=255};if e:=os.WriteFile(fmt.Sprintf("%s/go-%d.rgba",out,effect),pixels,0644);e!=nil{panic(e)}
 }}
''')
(OUT/'reference.go').write_text('\n'.join(parts))
subprocess.run(['docker','run','--rm','--network','none','-v',f'{OUT}:/out','betterpod-rust-build:1','sh','-ec','GOCACHE=/out/go-cache CGO_ENABLED=0 GO111MODULE=off go run /out/reference.go /out'],check=True)
env=os.environ.copy();env['BETTERPOD_SAVER_REFERENCE_OUT']=str(OUT)
if os.uname().sysname=='Darwin':
    frameworks=ROOT/'build/rust/macdeps/frameworks';env['RUSTFLAGS']=f'-L framework={frameworks} -C link-arg=-Wl,-rpath,{frameworks}'
subprocess.run(['cargo','test','--offline','--manifest-path',str(ROOT/'rust/Cargo.toml'),'--lib','saver::viz::tests::dump_reference_frames','--target-dir',str(ROOT/'build/rust-player-tests'),'--','--ignored','--exact'],env=env,check=True,stdout=subprocess.DEVNULL)
report={}
for effect,name in enumerate(['Bars','Waves','Tunnel','Stars','Spectrogram']):
    go=(OUT/f'go-{effect}.rgba').read_bytes();rust=(OUT/f'rust-{effect}.rgba').read_bytes();assert len(go)==len(rust)
    delta=[abs(a-b) for i,(a,b)in enumerate(zip(go,rust)) if i%4!=3]
    report[name]={'mean_rgb_error':sum(delta)/len(delta),'maximum_error':max(delta),'different_channels':sum(d>0 for d in delta),'channels':len(delta)}
(OUT/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
