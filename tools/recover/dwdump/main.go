package main

// dwdump: per-function DWARF summary (params, locals, inlined calls with call lines).
import (
	"debug/dwarf"
	"debug/elf"
	"fmt"
	"os"
	"strings"
)

func main() {
	f, err := elf.Open(os.Args[1])
	if err != nil { panic(err) }
	d, err := f.DWARF()
	if err != nil { panic(err) }
	if len(os.Args) > 2 && os.Args[2] == "layouts" { dumpLayouts(d); return }
	if len(os.Args) > 2 && os.Args[2] == "inl" { dumpInlineOnly(d); return }
	r := d.Reader()
	typeName := func(e *dwarf.Entry) string {
		off, ok := e.Val(dwarf.AttrType).(dwarf.Offset)
		if !ok { return "?" }
		t, err := d.Type(off)
		if err != nil { return "?" }
		return t.String()
	}
	var cuFiles []*dwarf.LineFile
	depth := 0
	inMain := false
	for {
		e, err := r.Next()
		if err != nil || e == nil { break }
		if e.Tag == 0 { depth--; if depth <= 1 { inMain = false }; continue }
		switch e.Tag {
		case dwarf.TagCompileUnit:
			lr, _ := d.LineReader(e)
			if lr != nil { cuFiles = lr.Files() } else { cuFiles = nil }
		case dwarf.TagSubprogram:
			name, _ := e.Val(dwarf.AttrName).(string)
			inMain = strings.HasPrefix(name, "main.")
			if inMain && e.Val(dwarf.AttrLowpc) != nil {
				fmt.Printf("\nFUNC %s\n", name)
			} else if inMain {
				inMain = false
			}
		case dwarf.TagFormalParameter, dwarf.TagVariable:
			if inMain {
				name, _ := e.Val(dwarf.AttrName).(string)
				kind := "var"
				if e.Tag == dwarf.TagFormalParameter { kind = "param" }
				line, _ := e.Val(dwarf.AttrDeclLine).(int64)
				fmt.Printf("%s%s %s %s @%d\n", strings.Repeat("  ", depth), kind, name, typeName(e), line)
			}
		case dwarf.TagInlinedSubroutine:
			if inMain {
				ao, _ := e.Val(dwarf.AttrAbstractOrigin).(dwarf.Offset)
				nm := "?"
				if ao != 0 {
					rr := d.Reader(); rr.Seek(ao); ae, _ := rr.Next()
					if ae != nil { nm, _ = ae.Val(dwarf.AttrName).(string) }
				}
				cl, _ := e.Val(dwarf.AttrCallLine).(int64)
				fmt.Printf("%sinline %s @%d\n", strings.Repeat("  ", depth), nm, cl)
			}
		case dwarf.TagLexDwarfBlock:
		}
		if e.Children { depth++ }
	}
	_ = cuFiles
}
