//go:build ignore

// Local reference only; copied into a scratch original Go tree.
package main
import("image";"image/color";"image/draw";"image/png";"os";"path/filepath";"fmt";"testing")
func TestWeatherIconReference(t *testing.T){out:=os.Getenv("BETTERPOD_WEATHER_ICONS_OUT");if out==""{t.Skip("local fixture path required")};os.MkdirAll(out,0755)
 for k:=0;k<11;k++{for _,size:=range []int{84,216}{dst:=image.NewRGBA(image.Rect(0,0,size,size));draw.Draw(dst,dst.Bounds(),&image.Uniform{color.RGBA{80,112,144,255}},image.Point{},draw.Src);icon:=wxIconDraw(wxKind(k),size,-1);draw.Draw(dst,dst.Bounds(),icon,image.Point{},draw.Over);f,e:=os.Create(filepath.Join(out,fmt.Sprintf("go-%d-%d.png",k,size)));if e!=nil{t.Fatal(e)};if e=png.Encode(f,dst);e!=nil{t.Fatal(e)};f.Close()}}
}
