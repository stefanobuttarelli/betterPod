#!/usr/bin/env python3
"""Deterministic synthetic fixtures and raw-pixel visual comparisons; stdlib only."""
from pathlib import Path
import argparse,hashlib,html,json,math,shutil,struct,wave,zlib

def png_write(path,w,h,rgba):
 def chunk(kind,data):return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
 raw=b''.join(b'\0'+bytes(rgba[y*w*4:(y+1)*w*4])for y in range(h))
 path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',w,h,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw,6))+chunk(b'IEND',b''))

def png_read(path):
 b=path.read_bytes();assert b[:8]==b'\x89PNG\r\n\x1a\n';offset=8;data=bytearray();shape=None
 while offset<len(b):
  n=struct.unpack_from('>I',b,offset)[0];kind=b[offset+4:offset+8];payload=b[offset+8:offset+8+n];offset+=12+n
  if kind==b'IHDR':shape=struct.unpack('>IIBBBBB',payload)
  elif kind==b'IDAT':data.extend(payload)
  elif kind==b'IEND':break
 w,h,depth,color,_,_,interlace=shape;assert depth==8 and color in(2,6)and interlace==0,(path,shape)
 channels=3 if color==2 else 4;stride=w*channels;raw=zlib.decompress(data);out=bytearray(w*h*4);previous=bytearray(stride);p=0
 def paeth(a,b,c):
  v=a+b-c;aa=abs(v-a);bb=abs(v-b);cc=abs(v-c);return a if aa<=bb and aa<=cc else b if bb<=cc else c
 for y in range(h):
  filtering=raw[p];p+=1;row=bytearray(raw[p:p+stride]);p+=stride
  for x in range(stride):
   left=row[x-channels]if x>=channels else 0;up=previous[x];corner=previous[x-channels]if x>=channels else 0
   predict=(0,left,up,(left+up)//2,paeth(left,up,corner))[filtering];row[x]=(row[x]+predict)&255
  for x in range(w):out[(y*w+x)*4:(y*w+x)*4+4]=row[x*channels:x*channels+channels]+(b'\xff'if channels==3 else b'')
  previous=row
 return w,h,out

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def fixtures(root):
 music=root/'fixtures/music';tracks=[]
 for album in range(8):
  folder=music/f'Visual Artist {album:02d}'/f'Visual Album {album:02d}';folder.mkdir(parents=True,exist_ok=True)
  pixels=bytearray()
  for y in range(256):
   for x in range(256):pixels.extend(((x+album*29)%256,(y+album*47)%256,((x+y)//2+album*67)%256,255))
  png_write(folder/'cover.png',256,256,pixels)
  for number in range(1,3):
   p=folder/f'{number:02d} Visual Track {album*2+number:02d}.wav'
   with wave.open(str(p),'wb')as f:f.setnchannels(2);f.setsampwidth(2);f.setframerate(44100);f.writeframes(b'\0'*4410*4)
   tracks.append(dict(file=str(p.relative_to(music)),cover=str((folder/'cover.png').relative_to(music)),title=f'Visual Track {album*2+number:02d}',artist=f'Visual Artist {album:02d}',album=f'Visual Album {album:02d}',number=number,duration=223.0+album*7+number-1))
 scenes=[dict(id=f'{i+1:02d}-{screen}',screen=screen,selected=selected,time_ms=1000)for i,(screen,selected)in enumerate([('home','Cover Flow'),('music','Cover Flow'),('albums','Visual Album 00'),('nowplaying',''),('coverflow','Visual Album 00'),('settings','Theme'),('sheet','Play Next'),('coverflow-card','Visual Track 01'),('nowplaying-card','Visual Track 01')])]
 for key in ['game2048','gamebrick','gamesolitaire','gamewords']:scenes.append(dict(id=f"{len(scenes)+1:02d}-{key}",screen=key,selected='',time_ms=1000))
 for route,screen,selected in [('home-music','music','Cover Flow'),('music-albums','albums','Visual Album 00'),('music-home','home','Music')]:
  for checkpoint in [0,75,150,225,300,350]:scenes.append(dict(id=f"{len(scenes)+1:02d}-{route}-{checkpoint:03d}ms",screen=screen,selected=selected,time_ms=1000+checkpoint,motion=route))
 for screen,selection in [('artists','Visual Artist 00'),('artist','Visual Album 00'),('album','Visual Track 01'),('songs','Visual Track 01'),('upnext','Visual Track 02'),('screensaver','Start Now'),('games','Brick'),('games','Solitaire'),('games','2048'),('games','Five Letters'),('games','Puzzles'),('settings','Shuffle'),('settings','Repeat'),('settings','Effects'),('settings','Turn it Apple'),('settings','Podcasts'),('settings','Sharing'),('settings','Quit betterPod'),('search','|'),('searchhub','Library')]:
  slug=selection.lower().replace(' ','-')or'keyboard';scenes.append(dict(id=f"{len(scenes)+1:02d}-{screen}-{slug}",screen=screen,selected=selection,time_ms=1000))
 for screen in ['maintenance-update-available','maintenance-move-copy','maintenance-move-verify']:scenes.append(dict(id=f"{len(scenes)+1:02d}-{screen}",screen=screen,selected='',time_ms=1000))
 for screen in ['curation-covers-ready','curation-covers-running','curation-covers-done']:scenes.append(dict(id=f"{len(scenes)+1:02d}-{screen}",screen=screen,selected='',time_ms=1000))
 for screen in ['video-playing','video-paused','video-loading']:scenes.append(dict(id=f"{len(scenes)+1:02d}-{screen}",screen=screen,selected='',time_ms=1000))
 for screen,selection in [('videos-home','Search YouTube'),('videos-saved','A short film'),('videos-recent','A short film'),('song-options','Play Next'),('album-options','Play Next')]:scenes.append(dict(id=f"{len(scenes)+1:02d}-{screen}",screen=screen,selected=selection,time_ms=1000))
 manifest=dict(schema=1,tracks=tracks,scenes=scenes,position=37.25,volume=.6,battery=73,wifi=4,clock='12:34',theme='light',effects='full',cover_order=list(range(8)))
 (root/'scenes.json').write_text(json.dumps(manifest,indent=2)+'\n')
 (root/'fixtures/files.json').write_text(json.dumps({str(p.relative_to(root/'fixtures')):sha(p)for p in sorted(music.rglob('*'))if p.is_file()},indent=2)+'\n')
 print(f'Fixture: {len(tracks)} tracks / 8 albums; regular files only; {root / "scenes.json"}')

def prepare_go(source,target):
 if target.exists():shutil.rmtree(target)
 target.mkdir(parents=True);shutil.copytree(source/'src',target/'src');shutil.copytree(source/'third_party',target/'third_party')
 for name in ['go.mod','go.sum']:shutil.copy2(source/name,target/name)
 hashes={str(p.relative_to(target)):sha(p)for p in sorted(target.rglob('*'))if p.is_file()}
 (target/'reference-source.json').write_text(json.dumps({'copied_source_sha256':hashes,'note':'Current Go1.1 source plus explicitly approved font kerning bugfix; capture watcher changes recorded separately'},indent=2)+'\n')
 ui=target/'src/ij_ui.go';text=ui.read_text();changes=[]
 for function in ['startBluetoothWatch','startWifiWatch','startHealthLog','startVolumeKeyWatch']:
  old='\t'+function+'()';assert old in text,function;text=text.replace(old,'\t// Visual-only diagnostic: '+function+' disabled.',1);changes.append(function)
 ui.write_text(text)
 shutil.copy2(source/'rust/tools/visual-parity-go-test.go',target/'src/visual_parity_test.go')
 shutil.copy2(source/'rust/tools/visual-parity-games-go-test.go',target/'src/visual_parity_games_test.go')
 shutil.copy2(source/'rust/tools/visual-parity-maintenance-go-test.go',target/'src/visual_parity_maintenance_test.go')
 (target/'diagnostic-patch.json').write_text(json.dumps({'disabled_constructor_watchers':changes,'rendering_functions_modified':False,'source_ui_sha256':sha(source/'src/ij_ui.go'),'diagnostic_ui_sha256':sha(ui)},indent=2)+'\n')
 destination=Path('/out') if Path('/out').is_dir()else source/'build/rust/visual-parity'
 if destination.is_dir():
  shutil.copy2(target/'reference-source.json',destination/'reference-source.json');shutil.copy2(target/'diagnostic-patch.json',destination/'diagnostic-patch.json')
 print('Prepared independent Go diagnostic source:',target)

def review_pair(path,left,right):
 # Labels are added outside the untouched640×480 captures; no resampling.
 glyphs={'G':[14,17,16,23,17,17,14],'O':[14,17,17,17,17,17,14],'R':[30,17,17,30,20,18,17],'U':[17,17,17,17,17,17,14],'S':[15,16,16,14,1,1,30],'T':[31,4,4,4,4,4,4],'6':[14,16,16,30,17,17,14],'4':[2,6,10,18,31,2,2],'0':[14,17,19,21,25,17,14],'X':[17,17,10,4,10,17,17],'8':[14,17,17,14,17,17,14],'1':[4,12,4,4,4,4,14],'.':[0,0,0,0,0,12,12],'|':[4,4,4,4,4,4,4]}
 width,height=1280,520;pixels=bytearray(bytes((28,31,36,255))*width*height)
 for side,(picture,label)in enumerate([(left,'GO 1.1 | 640 X 480'),(right,'RUST | 640 X 480')]):
  x=side*640+12
  for letter in label:
   for y,bits in enumerate(glyphs.get(letter,[0]*7)):
    for column in range(5):
     if bits&(1<<(4-column)):
      for dy in range(3):
       for dx in range(3):
        start=((9+y*3+dy)*width+x+column*3+dx)*4;pixels[start:start+4]=bytes((255,255,255,255))
   x+=18
  for y in range(480):
   start=((y+40)*width+side*640)*4;pixels[start:start+640*4]=picture[y*640*4:(y+1)*640*4]
 png_write(path,width,height,pixels)

def compare(root):
 manifest=json.loads((root/'scenes.json').read_text());out=root/'comparison';out.mkdir(exist_ok=True);rows=[];reports=[];contact=bytearray(1920*(len(manifest['scenes'])*480)*4)
 for row,s in enumerate(manifest['scenes']):
  ident=s['id'];paths=[root/'reference-1'/f'{ident}.png',root/'candidate-1'/f'{ident}.png'];decoded=[png_read(p)for p in paths];assert all((w,h)==(640,480)for w,h,_ in decoded)
  a=decoded[0][2];b=decoded[1][2];review_pair(root/'review'/f'{ident}.png',a,b);diff=bytearray(len(a));total=0;squares=0;changed=0;maximum=0;over2=0
  for i in range(0,len(a),4):
   values=[abs(a[i+c]-b[i+c])for c in range(3)];peak=max(values);changed+=peak>0;over2+=peak>2;total+=sum(values);squares+=sum(d*d for d in values);maximum=max(maximum,peak);diff[i:i+4]=bytes((min(255,values[0]*4),min(255,values[1]*4),min(255,values[2]*4),255))
  png_write(out/f'{ident}-diff.png',640,480,diff)
  states=[json.loads((p.parent/f'{ident}.state.json').read_text())for p in paths];keys=sorted(set(states[0])|set(states[1]));mismatches={k:{'go':states[0].get(k),'rust':states[1].get(k)}for k in keys if states[0].get(k)!=states[1].get(k)}
  repeat={side:sha(root/f'{side}-1'/f'{ident}.png')==sha(root/f'{side}-2'/f'{ident}.png')and (root/f'{side}-1'/f'{ident}.state.json').read_bytes()==(root/f'{side}-2'/f'{ident}.state.json').read_bytes()for side in ['reference','candidate']}
  report=dict(id=ident,deterministic=repeat,state_mismatches=mismatches,changed_rgb_pixels=changed,total_pixels=640*480,changed_above2=over2,maximum_channel_difference=maximum,mean_absolute_channel_difference=total/(640*480*3),rms_channel_difference=math.sqrt(squares/(640*480*3)),pixel_equal=changed==0)
  reports.append(report)
  for col,pixels in enumerate([a,b,diff]):
   for y in range(480):
    start=((row*480+y)*1920+col*640)*4;contact[start:start+640*4]=pixels[y*640*4:(y+1)*640*4]
  cells=''.join(f'<figure><figcaption>{html.escape(label)}</figcaption><img src="{html.escape(str(p.relative_to(root)))}"></figure>'for label,p in [('Go 1.1',paths[0]),('Rust',paths[1]),('Difference ×4',out/f'{ident}-diff.png')]);rows.append(f'<h2>{html.escape(ident)}</h2><p>Pixel differences: {changed}; state differences: {len(mismatches)}; deterministic: {repeat}</p><section>{cells}</section>')
 png_write(out/'contact-sheet.png',1920,len(manifest['scenes'])*480,contact)
 (root/'report.json').write_text(json.dumps({'scope':'visual comparison only; no performance or audio continuity claims','scenes':reports},indent=2)+'\n')
 (root/'index.html').write_text('<!doctype html><meta charset="utf-8"><title>Go1.1 / Rust visual parity</title><style>body{font:16px system-ui;background:#eee;color:#222}section{display:flex;gap:8px}figure{margin:0;width:32%}img{max-width:100%;image-rendering:auto}h2{margin-top:30px}</style><h1>Go1.1 / Rust — fixed state and time</h1>'+''.join(rows))
 print(json.dumps({'scenes':len(reports),'all_repeatable':all(all(x['deterministic'].values())for x in reports),'all_states_match':all(not x['state_mismatches']for x in reports),'all_pixels_equal':all(x['pixel_equal']for x in reports),'report':str(root/'report.json'),'contact_sheet':str(out/'contact-sheet.png')},indent=2))

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('command',choices=['fixtures','prepare-go','compare']);p.add_argument('--root',type=Path,default=Path('build/rust/visual-parity'));p.add_argument('--source',type=Path,default=Path('.'));p.add_argument('--target',type=Path);a=p.parse_args();a.root.mkdir(parents=True,exist_ok=True)
 if a.command=='fixtures':fixtures(a.root)
 elif a.command=='prepare-go':prepare_go(a.source,a.target or a.root/'source/go')
 else:compare(a.root)
