#!/usr/bin/env python3
"""Offline paired Go/Rust YouTube states using the existing visual harness."""
import argparse, importlib.util, json, shutil
from pathlib import Path
spec=importlib.util.spec_from_file_location('parity',Path(__file__).with_name('visual-parity.py'))
parity=importlib.util.module_from_spec(spec);spec.loader.exec_module(parity)

def fixtures(root):
 parity.fixtures(root)
 data=json.loads((root/'scenes.json').read_text())
 entries=[('youtube-search','Fixture Artist'),('youtube-gear','Fixture Artist'),('youtube-settings','Playlists & Albums'),('youtube-video-detail','Listen'),('youtube-playlist-detail','Listen'),('youtube-download',''),('youtube-row-loader','Fixture Video'),('youtube-download-error',''),('youtube-download-queued','')]
 data['scenes']=[dict(id=f'{i:02d}-{name}',screen=name,selected=selected,time_ms=1000)for i,(name,selected)in enumerate(entries,1)]
 (root/'scenes.json').write_text(json.dumps(data,indent=2)+'\n')
 print('Targeted scenes:',len(entries))

def prepare_go(source,target,root):
 parity.prepare_go(source,target)
 shutil.copy2(source/'rust/tools/youtube-design-go-test.go',target/'src/youtube_design_test.go')
 path=target/'src/ij_ytdl.go';text=path.read_text()
 old='}{thumbs: map[string]*Album{}}';assert old in text
 text=text.replace(old,old+'\n\tqaYTSeedSearch(&st.term, &st.ran, &st.busy, &st.playlists, &st.res)',1)
 old='st := &ytItemState{}';assert old in text
 text=text.replace(old,old+'\n\tif qaYTInfo != nil { st.info = qaYTInfo; st.coverTried = true }',1)
 old='\tload()\n\treturn v\n}\n\ntype ytItemState';assert old in text
 text=text.replace(old,'\tif qaYTInfo == nil { load() }\n\treturn v\n}\n\ntype ytItemState',1)
 text=text.replace('go ffmpegReady() // (checked before a video is chosen: it takes a moment)','// Offline diagnostic: no tool probe is started.')
 path.write_text(text)
 path=target/'src/visual_parity_test.go';text=path.read_text()
 old='default:if !qaSeedMaintenance';assert old in text
 text=text.replace(old,'default:if !qaSeedYouTube(t,ui,scene.Screen)&&!qaSeedMaintenance')
 old='for i:=0;i<2;i++{ui.renderFrame(now)}';assert old in text
 text=text.replace(old,old+';qaYTPostDraw(ui,scene.Screen)')
 path.write_text(text)
 (root/'youtube-fixture-patch.json').write_text(json.dumps(dict(scope='Scratch-copy fixture state only; product Go and Rust renderers unchanged.',requested_overrides=['Cancel is selected blue while active or queued','Waiting job shows queue position and B keeps it queued'],network='Docker --network none',hooks=['Seed actual youtubeSearchView state','Seed actual ytItemView info; suppress its network load','Construct actual iaDownloadView state','Overlay canonical Go drawSpinnerIn for the new pressed-row feedback state']),indent=2)+'\n')

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('command',choices=['fixtures','prepare-go','compare']);p.add_argument('--root',type=Path,required=True);p.add_argument('--source',type=Path,default=Path('.'));p.add_argument('--target',type=Path);a=p.parse_args();a.root.mkdir(parents=True,exist_ok=True)
 if a.command=='fixtures':fixtures(a.root)
 elif a.command=='prepare-go':prepare_go(a.source,a.target,a.root)
 else:parity.compare(a.root)
