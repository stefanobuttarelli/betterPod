package main

import (
	"debug/elf"
	"debug/gosym"
	"fmt"
	"os"
	"sort"
	"strings"
)

func main() {
	f, err := elf.Open(os.Args[1])
	if err != nil { panic(err) }
	pcln := f.Section(".gopclntab")
	data, _ := pcln.Data()
	text := f.Section(".text")
	lt := gosym.NewLineTable(data, text.Addr)
	tab, err := gosym.NewTable(nil, lt)
	if err != nil { panic(err) }
	type fn struct{ name, file string; l0, l1 int }
	var fs []fn
	for _, s := range tab.Funcs {
		if !strings.HasPrefix(s.Name, "main.") { continue }
		file, l0, _ := tab.PCToLine(s.Entry)
		// scan pcs for max line
		l1 := l0; lmin := l0
		for pc := s.Entry; pc < s.End; pc += 4 {
			ff, l, _ := tab.PCToLine(pc)
			if ff == file { if l > l1 { l1 = l }; if l < lmin && l > 0 { lmin = l } }
		}
		fs = append(fs, fn{s.Name, file, lmin, l1})
	}
	sort.Slice(fs, func(i, j int) bool { if fs[i].file != fs[j].file { return fs[i].file < fs[j].file }; return fs[i].l0 < fs[j].l0 })
	for _, x := range fs {
		i := strings.LastIndex(x.file, "/")
		fmt.Printf("%s\t%d\t%d\t%s\n", x.file[i+1:], x.l0, x.l1, x.name)
	}
}
