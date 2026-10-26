// dwarfvars prints, for every main.* function in an unstripped binary, its
// parameters, local variables (with declaration line) and inlined calls.
package main

import (
	"debug/dwarf"
	"debug/elf"
	"fmt"
	"os"
	"strings"
)

func main() {
	f, err := elf.Open(os.Args[1])
	if err != nil {
		panic(err)
	}
	d, err := f.DWARF()
	if err != nil {
		panic(err)
	}
	if len(os.Args) > 2 && os.Args[2] == "structs" {
		dumpStructs(d)
		return
	}
	r := d.Reader()
	tname := func(e *dwarf.Entry) string {
		off, ok := e.Val(dwarf.AttrType).(dwarf.Offset)
		if !ok {
			return "?"
		}
		t, err := d.Type(off)
		if err != nil {
			return "?"
		}
		return t.String()
	}
	originName := func(e *dwarf.Entry) string {
		off, ok := e.Val(dwarf.AttrAbstractOrigin).(dwarf.Offset)
		if !ok {
			return ""
		}
		rr := d.Reader()
		rr.Seek(off)
		oe, err := rr.Next()
		if err != nil || oe == nil {
			return ""
		}
		n, _ := oe.Val(dwarf.AttrName).(string)
		return n
	}
	depth := 0
	inMain := false
	for {
		e, err := r.Next()
		if err != nil || e == nil {
			break
		}
		if e.Tag == 0 {
			depth--
			if depth <= 1 {
				inMain = false
			}
			continue
		}
		if e.Tag == dwarf.TagSubprogram && depth == 1 {
			name, _ := e.Val(dwarf.AttrName).(string)
			inl, _ := e.Val(dwarf.AttrInline).(int64)
			if strings.HasPrefix(name, "main.") {
				inMain = true
				kind := ""
				if inl != 0 {
					kind = " (abstract)"
				}
				fmt.Printf("FUNC %s%s\n", name, kind)
			}
			if e.Children {
				depth++
			}
			continue
		}
		if inMain {
			ind := strings.Repeat("  ", depth)
			switch e.Tag {
			case dwarf.TagFormalParameter, dwarf.TagVariable:
				name, _ := e.Val(dwarf.AttrName).(string)
				if name == "" {
					name = originName(e)
				}
				line, _ := e.Val(dwarf.AttrDeclLine).(int64)
				k := "var"
				if e.Tag == dwarf.TagFormalParameter {
					k = "param"
				}
				fmt.Printf("%s%s %s %s @%d\n", ind, k, name, tname(e), line)
			case dwarf.TagInlinedSubroutine:
				line, _ := e.Val(dwarf.AttrCallLine).(int64)
				fmt.Printf("%sinline %s @%d\n", ind, originName(e), line)
			case dwarf.TagLexDwarfBlock:
				fmt.Printf("%sblock\n", ind)
			}
		}
		if e.Children {
			depth++
		}
	}
}
