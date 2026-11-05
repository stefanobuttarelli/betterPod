#!/usr/bin/env python3
"""Native-resolution Go/Rust weather path fixtures, local and without network."""
from pathlib import Path
import importlib.util,json,os,subprocess
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'build/rust/weather-icons';OUT.mkdir(parents=True,exist_ok=True)
subprocess.run(['docker','run','--rm','--network','none','-v',f'{ROOT}:/repo:ro','-v',f'{OUT}:/out','-v','betterpod-gomod:/go/pkg/mod','-v','betterpod-visual-gocache:/root/.cache/go-build','-e','SDL_AUDIODRIVER=dummy','-e','BETTERPOD_WEATHER_ICONS_OUT=/out','betterpod-rust-build:1','sh','-ec',r'''
 python3 /repo/rust/tools/visual-parity.py fixtures --root /out
 python3 /repo/rust/tools/visual-parity.py prepare-go --source /repo --target /w/go --root /out
 ln -s /repo/port/betterpod/assets /w/go/src/assets
 tail -n +3 /repo/rust/tools/weather-icons-go-test.go > /w/go/src/weather_icons_reference_test.go
 export LD_LIBRARY_PATH=/repo/port/betterpod/libs
 cd /w/go/src
 CGO_CFLAGS=-I/usr/include/SDL2 go test -count=1 -run '^TestWeatherIconReference$' *.go
'''],check=True)
env=os.environ.copy();env['BETTERPOD_WEATHER_ICONS_OUT']=str(OUT)
if os.uname().sysname=='Darwin':
 frameworks=ROOT/'build/rust/macdeps/frameworks';env['RUSTFLAGS']=f'-L framework={frameworks} -C link-arg=-Wl,-rpath,{frameworks}'
shim=OUT/'rust-check';shim.mkdir(exist_ok=True)
(shim/'Cargo.toml').write_text('''[package]
name="betterpod-weather-reference-check"
version="0.0.0"
edition="2021"
[lib]
path="lib.rs"
[dependencies]
tiny-skia={version="=0.12.0",default-features=false,features=["std","simd"]}
glow="0.18"
fontdue="0.9"
ttf-parser="0.25.1"
flate2="1"
image={version="0.25",default-features=false,features=["png"]}
[target.'cfg(target_os="macos")'.dependencies]
sdl2={version="0.38",features=["mixer","use_mac_framework"]}
[target.'cfg(not(target_os="macos"))'.dependencies]
sdl2={version="0.38",features=["mixer"]}
''')
(shim/'lib.rs').write_text('\n'.join(f'#[path="{ROOT}/rust/src/{name}.rs"]pub mod {name};'for name in ['render','font_metrics','weather_icons'])+'\n')
subprocess.run(['cargo','test','--offline','--manifest-path',str(shim/'Cargo.toml'),'--lib','weather_icons::tests::dump_reference_icons','--target-dir',str(ROOT/'build/rust-player-tests'),'--','--exact','--ignored'],env=env,check=True,stdout=subprocess.DEVNULL)
spec=importlib.util.spec_from_file_location('visual',ROOT/'rust/tools/visual-parity.py');visual=importlib.util.module_from_spec(spec);spec.loader.exec_module(visual)
report={};names=['Sun','Moon','PartlySun','PartlyMoon','Cloudy','Fog','Drizzle','Rain','Snow','Sleet','Thunder']
for i,name in enumerate(names):
 for size in [84,216]:
  _,_,a=visual.png_read(OUT/f'go-{i}-{size}.png');_,_,b=visual.png_read(OUT/f'rust-{i}-{size}.png');delta=[abs(x-y) for n,(x,y) in enumerate(zip(a,b)) if n%4!=3];report[f'{name}-{size}']={'mean_rgb_error':sum(delta)/len(delta),'max_error':max(delta)}
(OUT/'comparison.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
