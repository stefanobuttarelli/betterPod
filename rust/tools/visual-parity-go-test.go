//go:build ignore

// Copied into a scratch Go 1.1 tree by visual-parity.sh. Not linked into Rust.
package main

import (
 "encoding/json"
 "fmt"
 "image/png"
 "os"
 "path/filepath"
 "strings"
 "sync/atomic"
 "testing"
 "time"
)
type qaTrack struct { File string `json:"file"`; Cover string `json:"cover"`; Title string `json:"title"`; Artist string `json:"artist"`; Album string `json:"album"`; Number int `json:"number"`; Duration float64 `json:"duration"` }
type qaScene struct { Motion string `json:"motion"`; ID string `json:"id"`; Screen string `json:"screen"`; Selected string `json:"selected"`; TimeMS int64 `json:"time_ms"` }
type qaManifest struct { Tracks []qaTrack `json:"tracks"`; Scenes []qaScene `json:"scenes"`; Position float64 `json:"position"`; Volume float64 `json:"volume"`; Battery int `json:"battery"`; Wifi int `json:"wifi"`; Clock string `json:"clock"` }
func qaReadManifest(t *testing.T)qaManifest { t.Helper();b,e:=os.ReadFile(os.Getenv("BETTERPOD_VISUAL_MANIFEST"));if e!=nil{t.Fatal(e)};var m qaManifest;if e=json.Unmarshal(b,&m);e!=nil{t.Fatal(e)};return m }
func qaLibrary(t *testing.T,m qaManifest)*Library {
 t.Helper();lib:=&Library{TracksByPath:map[string]*Track{},AlbumsByKey:map[string]*Album{},ArtistsByName:map[string]*Artist{}};root:=os.Getenv("BETTERPOD_VISUAL_FIXTURES")
 for _,s:=range m.Tracks {p:=filepath.Join(root,"music",s.File);tr:=&Track{Path:p,Title:s.Title,Artist:s.Artist,Album:s.Album,AlbumArtist:s.Artist,TrackNum:s.Number,Duration:s.Duration,SampleRate:44100,HasArt:true};lib.Tracks=append(lib.Tracks,tr);lib.TracksByPath[p]=tr;key:=albumKeyFor(tr);al:=lib.AlbumsByKey[key];if al==nil{al=&Album{Name:s.Album,Artist:s.Artist,Key:key,ArtPath:filepath.Join(root,"music",s.Cover)};f,e:=os.Open(al.ArtPath);if e!=nil{t.Fatal(e)};im,e:=png.Decode(f);f.Close();if e!=nil{t.Fatal(e)};al.ArtImg=im;lib.Albums=append(lib.Albums,al);lib.AlbumsByKey[key]=al;ar:=lib.ArtistsByName[s.Artist];if ar==nil{ar=&Artist{Name:s.Artist};lib.Artists=append(lib.Artists,ar);lib.ArtistsByName[s.Artist]=ar};ar.Albums=append(ar.Albums,al)};al.Tracks=append(al.Tracks,tr)
 };return lib
}
func qaSelect(t *testing.T,v *View,label string){t.Helper();if label==""{return};for i,it:=range v.Items{if it.Label==label{v.Index=i;v.dirty=true;return}};t.Fatalf("Go scene missing %q in %q",label,v.ID)}
func qaDrain(ui *IpodUI){for{select{case f:=<-ui.events:f();default:return}}}
func TestVisualParitySnapshots(t *testing.T){
 if os.Getenv("BETTERPOD_VISUAL_MANIFEST")==""{t.Skip("local visual manifest required")};m:=qaReadManifest(t);out:=os.Getenv("BETTERPOD_VISUAL_OUTPUT");if e:=os.MkdirAll(out,0755);e!=nil{t.Fatal(e)}
 epoch:=time.Date(2026,11,5,12,34,56,0,time.UTC);var now=epoch;oldNow:=animNow;animNow=func()time.Time{return now};defer func(){animNow=oldNow}()
 lib:=qaLibrary(t,m);for _,track:=range lib.Tracks{trackFormats[track]=probeFormat(track.Path)};themeMode="light";effectsMode="full";appleMode=false;podsInMenu=true;ytEnabled=false;saverAfter=0;saverDim=false;saverStyle="cover"
 fmt.Println("TEXT_WIDTH automatic=",textWidth(face(W400,12),"Dark from 20:00 to 7:00"));for _,scene:=range m.Scenes {t.Run(scene.ID,func(t *testing.T){
  now=epoch;pausedSince=time.Time{};pauseHideFired=false;npOpening.Store(false);mini=miniState{show:map[ViewKind]*Anim{KindSplit:newAnim(1),KindFull:newAnim(1)},pause:newAnim(0),change:newAnim(1),shown:lib.Tracks[0],dir:1,since:epoch};colPreview.parent=nil;colPreview.v=nil
  app:=&BetterPod{Library:lib,Queue:&PlaybackQueue{Tracks:lib.Tracks,CurrentIndex:0,Repeat:RepeatOff},Playing:&NowPlaying{Track:lib.Tracks[0],State:StatePlaying,Position:m.Position,Duration:lib.Tracks[0].Duration,Volume:m.Volume},RefreshChan:make(chan struct{},16),RedrawChan:make(chan struct{},16),Running:false,LastActivityTime:epoch}
  globalApp=app;ui:=newIpodUI(app);ipodUI=ui;if scene.ID=="01-home"{qaExportIcons(t,ui,out)};defer ui.artc.close();ui.noMarquee=true;ui.volume=m.Volume;ui.shuffleMode="off";ui.kb=&KenBurns{stack:append([]*Album{},lib.Albums...),animStart:map[*Album]time.Time{lib.Albums[0]:epoch},mounted:epoch,lastCycle:epoch,initialized:true}
  // Synchronous preparation uses the real Go scaling/cache, not altered drawing.
  ui.artc.start.Do(func(){});ui.artc.mu.Lock();for _,al:=range lib.Albums{for _,size:=range []int{48,64,80,96,112,128,140,160,192,240,256,320,384,440,480,512,640}{ui.artc.put(artKey{al,size},scaleImage(al.ArtImg,size,size))}};ui.artc.mu.Unlock()
  if scene.Motion!="" {
    qaSelect(t,ui.topScreen(),"Music");if scene.Motion!="home-music"{ui.showView(ui.musicView(),"");qaSelect(t,ui.topScreen(),"Albums")}
  }else{switch scene.Screen {
  case "home":qaSelect(t,ui.split[len(ui.split)-1],scene.Selected)
  case "music":ui.showView(ui.musicView(),"");qaSelect(t,ui.split[len(ui.split)-1],scene.Selected)
  case "albums","sheet":ui.showView(ui.musicView(),"");ui.showView(ui.albumsView(nil),"");if scene.Screen=="sheet"{ui.showSheetTitled(m.Tracks[0].Title,[]SheetOption{{Label:"Play Next",Icon:"queue_next"},{Label:"Play Later",Icon:"queue_last"}});ui.sheets[0].anim.Set(0)}
  case "song-options":ui.showView(ui.songsView(lib.Tracks),"");ui.showSheetTitled(lib.Tracks[0].Title,ui.mediaOptions([]*Track{lib.Tracks[0]}));ui.sheets[0].anim.Set(0)
  case "album-options":ui.showView(ui.albumsView(nil),"");ui.showSheetTitled(lib.Albums[0].Name,ui.albumOptions(lib.Albums[0]));ui.sheets[0].anim.Set(0)
  case "settings":ui.showView(ui.settingsView(),"");qaSelect(t,ui.split[len(ui.split)-1],scene.Selected)
  case "artists":ui.showView(ui.artistsView(),"");qaSelect(t,ui.topScreen(),scene.Selected)
  case "artist":ui.showView(ui.artistView(lib.Artists[0]),"");qaSelect(t,ui.topScreen(),scene.Selected)
  case "album":ui.showView(ui.albumView(lib.Albums[0]),"");qaSelect(t,ui.topScreen(),scene.Selected)
  case "songs":ui.showView(ui.songsView(lib.Tracks),"");qaSelect(t,ui.topScreen(),scene.Selected)
  case "upnext":ui.showView(ui.upNextView(),"");qaSelect(t,ui.topScreen(),scene.Selected)
  case "screensaver":ui.showView(ui.saverSettingsView(),"");qaSelect(t,ui.topScreen(),scene.Selected)
  case "games":ui.showView(ui.gamesView(),"");qaSelect(t,ui.topScreen(),scene.Selected)
  case "search":ui.showView(ui.searchView(),"");ui.keyboard.anim.Set(1)
  case "searchhub":ui.showView(ui.searchHubView(),"");qaSelect(t,ui.topScreen(),scene.Selected)
  case "nowplaying","nowplaying-card":ui.showNowPlaying();np:=ui.topScreen().Custom.(*NowPlayingView);np.barsLive.Set(1);if scene.Screen=="nowplaying-card"{np.toggleCard(ui);np.flip.Set(1);np.backA.Set(1)}
  case "coverflow","coverflow-card":ui.openCoverFlow();if scene.Screen=="coverflow-card"{ui.cover.center(ui);ui.cover.backAnim.Set(1)};ui.cover.active=0;ui.cover.fadeIn.Set(1);ui.cover.sync(epoch);for _,c:=range ui.cover.covers{c.from=c.to;c.dur=0;c.opacity.Set(1);c.artAlpha.Set(1)}
  default:if !qaSeedMaintenance(t,ui,scene.Screen,os.Getenv("BETTERPOD_VISUAL_FIXTURES"))&&!qaSeedGame(t,ui,scene.Screen,os.Getenv("BETTERPOD_VISUAL_FIXTURES")){t.Fatal("unsupported scene",scene.Screen)}
  }}
  if ui.previewTimer!=nil{ui.previewTimer.Stop()};desired:=PrevMusic;if len(ui.split)>0{v:=ui.split[len(ui.split)-1];if v.Index>=0&&v.Index<len(v.Items){desired=v.Items[v.Index].Preview}};ui.preview=desired;ui.pendingPrev=desired;ui.finishSlides();qaDrain(ui)
  now=epoch.Add(time.Duration(scene.TimeMS)*time.Millisecond);atomic.StoreInt32(&wifiState,int32(m.Wifi));batMu.Lock();batPct=m.Battery;batChg=false;batChecked=time.Now().Add(24*time.Hour);batMu.Unlock()
  // Preseed targets so every static frame is the same endpoint independent of rendering history.
  ui.followPause(now);ui.followTicker(now);ui.tickOverlays(now);for kind,a:=range mini.show{var top *View;if kind==KindSplit&&len(ui.split)>0{top=ui.split[len(ui.split)-1]}else if kind==KindFull&&len(ui.full)>0{top=ui.full[len(ui.full)-1]};target:=0.0;if top!=nil&&ui.miniReserve(top){target=1};a.Set(target)};mini.change.Set(1);mini.pause.Set(0)
  if scene.Motion!="" {
    now=epoch.Add(time.Second);for i:=0;i<2;i++{ui.renderFrame(now)}
    if scene.Motion=="music-home"{ui.hideView()}else{ui.centerClick()}
    if ui.previewTimer!=nil{ui.previewTimer.Stop()};qaDrain(ui)
    now=epoch.Add(time.Duration(scene.TimeMS)*time.Millisecond)
  }
  for i:=0;i<2;i++{ui.renderFrame(now)}
  p:=filepath.Join(out,scene.ID+".png");f,e:=os.Create(p);if e!=nil{t.Fatal(e)};if e=png.Encode(f,ui.fb);e!=nil{t.Fatal(e)};f.Close()
  actual:=strings.ToLower(ui.topScreen().ID);selection:="";rowLabels:=[]string{}
  if ui.coverUp()!=nil{actual="coverflow";selection=ui.cover.albums[ui.cover.active].Name;if ui.cover.selected!=nil{actual="coverflow-card";selection=ui.cover.back.Items[ui.cover.back.Index].Label}}else if np,ok:=ui.topScreen().Custom.(*NowPlayingView);ok&&np.flipped{actual="nowplaying-card";selection=np.back.Items[np.back.Index].Label}else if sh:=ui.activeSheet();sh!=nil{actual="sheet";selection=sh.opts[sh.index].Label;for _,o:=range sh.opts{rowLabels=append(rowLabels,o.Label)}}else{v:=ui.topScreen();if v.Custom==nil&&v.Index>=0&&v.Index<len(v.Items){selection=v.Items[v.Index].Label};for _,it:=range v.Items{rowLabels=append(rowLabels,it.Label)}}
  if (scene.Screen=="song-options"||scene.Screen=="album-options")&&actual=="sheet"{actual=scene.Screen}
  if actual!=scene.Screen||selection!=scene.Selected{t.Fatalf("actual state %s/%s != requested %s/%s",actual,selection,scene.Screen,scene.Selected)}
  controlMode:="none";if np,ok:=ui.topScreen().Custom.(*NowPlayingView);ok{controlMode="progress";if np.flipped{controlMode="card"}else if np.scrubbing{controlMode="scrub"}else if np.opts{controlMode="options"}};miniKind:=KindFull;if ui.topScreen().Kind==KindSplit{miniKind=KindSplit};miniWanted:=ui.miniReserve(ui.topScreen());miniShown:=mini.show[miniKind].Get(now);if ui.coverUp()!=nil{miniWanted=false;miniShown=0;controlMode="none";if ui.cover.selected!=nil{controlMode="card"}};state:=map[string]any{"id":scene.ID,"screen":actual,"selected":selection,"scroll":ui.topScreen().Scroll,"rows":rowLabels,"time_ms":scene.TimeMS,"track":m.Tracks[0].Title,"position":m.Position,"duration":m.Tracks[0].Duration,"playing":true,"lossless":trackFormats[lib.Tracks[0]].lossless,"mini_wanted":miniWanted,"mini_shown":miniShown,"control_mode":controlMode,"clock":m.Clock,"battery":m.Battery,"wifi":m.Wifi,"theme":"light","effects":"full","tracks":len(lib.Tracks),"albums":len(lib.Albums)};b,_:=json.MarshalIndent(state,"","  ");if e=os.WriteFile(filepath.Join(out,scene.ID+".state.json"),b,0644);e!=nil{t.Fatal(e)};fmt.Println("CAPTURE",scene.ID,p)
 })}
}
