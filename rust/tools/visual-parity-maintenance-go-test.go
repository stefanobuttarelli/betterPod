//go:build ignore

// Copied only into the isolated Go reference test tree. No work is started.
package main

import (
 "path/filepath"
 "os"
 "encoding/json"
 "sort"
 "testing"
 "time"
)

func qaSeedMaintenance(t *testing.T, ui *IpodUI, screen, fixtureRoot string) bool {
 t.Helper()
 switch screen {
 case "videos-home", "videos-saved", "videos-recent":
  ffmpegCheck.mu.Lock();ffmpegCheck.done=true;ffmpegCheck.ok=true;ffmpegCheck.lacks="";ffmpegCheck.mu.Unlock()
  if err:=os.MkdirAll(videosDir(),0755);err!=nil{t.Fatal(err)}
  saved:=[]videoSaved{{ID:"fixture_first",Title:"A short film",Channel:"Fixture Studio",Duration:245,Size:12345678,At:20},{ID:"fixture_second",Title:"Another journey",Channel:"Fixture Studio",Duration:130,Size:23456789,At:10}}
  for _,record:=range saved {b,_:=json.Marshal(record);if err:=os.WriteFile(videoSavedPath(record.ID,".json"),b,0644);err!=nil{t.Fatal(err)};if err:=os.WriteFile(videoSavedPath(record.ID,".mp4"),[]byte("private synthetic video fixture"),0644);err!=nil{t.Fatal(err)}}
  videoSaveHistory([]videoSeen{{ID:"fixture_first",Title:"A short film",Channel:"Fixture Studio",Duration:245,Pos:65,At:20},{ID:"fixture_second",Title:"Another journey",Channel:"Fixture Studio",Duration:130,Pos:0,At:10}})
  var view *View;switch screen{case "videos-home":view=ui.videosView();case "videos-saved":view=ui.savedVideosView();case "videos-recent":view=ui.recentVideosView()};view.ID=screen;ui.showView(view,"");return true
 case "video-playing", "video-paused", "video-loading":
  vv:=&VideoView{e:ytEntry{Title:"A short film",Duration:245},ctl:newAnim(1),flash:newAnim(0),ctlUntil:animNow().Add(2500*time.Millisecond),timed:true}
  if screen=="video-loading"{vv.load=newYTLoading();vv.load.start=time.Now().Add(-2*time.Second)}else{
   pixels:=make([]byte,640*360*4);for y:=0;y<360;y++{for x:=0;x<640;x++{i:=(y*640+x)*4;pixels[i]=byte(x*255/639);pixels[i+1]=byte(y*255/359);pixels[i+2]=byte(96+((x/40+y/40)%2)*32);pixels[i+3]=255}}
   vv.pl=&videoPlayer{src:vSource{Duration:245},fw:640,fh:360,state:vPlaying,paused:screen=="video-paused",pos:65,shown:pixels}
  }
  view:=&View{ID:screen,Title:"A short film",Kind:KindFull,Custom:vv};vv.v=view;ui.showView(view,"");return true
 case "curation-covers-ready", "curation-covers-running", "curation-covers-done":
  // A private coverless copy keeps later golden scenes independent.
  library:=*ui.library();library.Albums=nil;library.AlbumsByKey=map[string]*Album{}
  for _,album:=range ui.library().Albums {copy:=*album;copy.ArtData=nil;copy.ArtImg=nil;copy.ArtPath="";library.Albums=append(library.Albums,&copy);library.AlbumsByKey[copy.Key]=&copy}
  ui.app.Library=&library;coverCur=nil
  if screen!="curation-covers-ready" {
   n:=len(library.Albums);j:=&coverJob{albums:library.Albums,cancel:make(chan struct{})};j.index=min(1,max(0,n-1));if n>1{j.found=1};if n>0{j.current=library.Albums[j.index]}
   if screen=="curation-covers-done" {j.finished=true;j.index=n;j.current=nil;if n>0{j.found=1};for _,album:=range library.Albums[j.found:]{j.missed=append(j.missed,album.Name)};j.failed=len(j.missed)}
   coverCur=j
  }
  ui.showView(&View{ID:screen,Title:"Find Missing Covers",Kind:KindFull,Custom:&CoverView{}},"");return true
 case "maintenance-update-available":
  upd.state=updAvailable
  upd.manifest=&updateManifest{Version:"1.2",Size:12345678,Notes:"Playback and interface refinements\nMore reliable Bluetooth playback\nImproved album browsing"}
  view:=ui.updateView();view.ID=screen;ui.showView(view,"")
  return true
 case "maintenance-move-copy", "maintenance-move-verify":
  job:=&moveJob{from:filepath.Join(fixtureRoot,"music"),to:"/fixture-sd2/music",fromName:"SD 1",toName:"SD 2",move:true,phase:"copy",speed:8*1024*1024,stop:make(chan struct{})}
  for _,album:=range ui.library().Albums {
   unit:=&moveUnit{album:album}
   for _,track:=range album.Tracks {relative,_:=filepath.Rel(job.from,track.Path);unit.dir=filepath.Dir(relative);unit.files=append(unit.files,&moveFile{rel:relative,size:10*1024*1024});unit.bytes+=10*1024*1024;job.files++;job.songs++}
   if len(unit.files)>0 {job.units=append(job.units,unit);job.all+=unit.bytes}
  }
  sort.Slice(job.units,func(a,b int)bool{return job.units[a].dir<job.units[b].dir})
  if len(job.units)==0 {t.Fatal("Moving Music visual fixture needs albums")}
  job.cur=min(1,len(job.units)-1)
  for _,unit:=range job.units[:job.cur] {job.doneB+=unit.bytes;job.checked+=len(unit.files)}
  unit:=job.units[job.cur];job.unitDone=unit.bytes/2;job.doneB+=job.unitDone;job.checked+=len(unit.files)/2;job.file=filepath.Base(unit.files[0].rel)
  if screen=="maintenance-move-verify"{job.phase="verify"}
  view:=&View{ID:screen,Title:"Moving Music",Kind:KindFull,Custom:&moveView{job:job,shownUnit:job.cur,shownFrac:0.5,last:animNow()}}
  ui.showView(view,"");return true
 }
 return false
}
