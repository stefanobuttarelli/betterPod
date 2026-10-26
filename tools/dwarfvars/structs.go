package main

import (
	"debug/dwarf"
	"fmt"
	"strings"
)

// dumpStructs prints the field offsets of every main.* struct type.
func dumpStructs(d *dwarf.Data) {
	r := d.Reader()
	for {
		e, err := r.Next()
		if err != nil || e == nil {
			break
		}
		if e.Tag != dwarf.TagStructType {
			continue
		}
		name, _ := e.Val(dwarf.AttrName).(string)
		if !strings.HasPrefix(name, "main.") {
			r.SkipChildren()
			continue
		}
		t, err := d.Type(e.Offset)
		if err != nil {
			continue
		}
		st, ok := t.(*dwarf.StructType)
		if !ok {
			continue
		}
		fmt.Printf("STRUCT %s %d\n", name, st.ByteSize)
		for _, f := range st.Field {
			fmt.Printf("  %d %s %s\n", f.ByteOffset, f.Name, f.Type.String())
		}
		r.SkipChildren()
	}
}
