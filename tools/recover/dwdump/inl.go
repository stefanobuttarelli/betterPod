package main

import (
	"debug/dwarf"
	"fmt"
	"strings"
)

func dumpInlineOnly(d *dwarf.Data) {
	r := d.Reader()
	concrete := map[string]bool{}
	var abs []string
	for {
		e, err := r.Next()
		if err != nil || e == nil {
			break
		}
		if e.Tag == dwarf.TagSubprogram {
			n, _ := e.Val(dwarf.AttrName).(string)
			if strings.HasPrefix(n, "main.") {
				if e.Val(dwarf.AttrLowpc) != nil {
					concrete[n] = true
				} else if e.Val(dwarf.AttrInline) != nil {
					abs = append(abs, n)
				}
			}
			r.SkipChildren()
		}
	}
	for _, n := range abs {
		if !concrete[n] {
			fmt.Println(n)
		}
	}
}
