package main

import (
	"debug/dwarf"
	"fmt"
	"strings"
)

// dumpLayouts prints struct layouts of main.* types and the types of main.* globals.
func dumpLayouts(d *dwarf.Data) {
	r := d.Reader()
	seen := map[dwarf.Offset]bool{}
	var printStruct func(name string, t dwarf.Type, ind string)
	printStruct = func(name string, t dwarf.Type, ind string) {
		st, ok := t.(*dwarf.StructType)
		if !ok {
			return
		}
		fmt.Printf("%sSTRUCT %s size=%d\n", ind, name, st.ByteSize)
		for _, f := range st.Field {
			fmt.Printf("%s  +%d %s %s\n", ind, f.ByteOffset, f.Name, f.Type.String())
		}
	}
	for {
		e, err := r.Next()
		if err != nil || e == nil {
			break
		}
		name, _ := e.Val(dwarf.AttrName).(string)
		switch e.Tag {
		case dwarf.TagStructType:
			if strings.HasPrefix(name, "main.") && !seen[e.Offset] {
				seen[e.Offset] = true
				t, err := d.Type(e.Offset)
				if err == nil {
					printStruct(name, t, "")
				}
			}
			r.SkipChildren()
		case dwarf.TagVariable:
			if strings.HasPrefix(name, "main.") {
				off, _ := e.Val(dwarf.AttrType).(dwarf.Offset)
				t, err := d.Type(off)
				if err == nil {
					fmt.Printf("GLOBAL %s %s\n", name, t.String())
					if st, ok := t.(*dwarf.StructType); ok && !strings.HasPrefix(st.StructName, "main.") {
						printStruct(name, t, "  ")
					}
				}
			}
		case dwarf.TagSubprogram:
			r.SkipChildren()
		}
	}
}
