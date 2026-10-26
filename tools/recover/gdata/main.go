// gdata: print initial (static) data of main.* globals from an ELF.
// usage: gdata dbg.elf regex
package main

import (
	"debug/elf"
	"encoding/binary"
	"fmt"
	"math"
	"os"
	"regexp"
	"sort"
)

func main() {
	f, err := elf.Open(os.Args[1])
	if err != nil {
		panic(err)
	}
	if os.Args[2] == "mem" {
		var addr, n uint64
		fmt.Sscanf(os.Args[3], "%x", &addr)
		fmt.Sscanf(os.Args[4], "%d", &n)
		for _, sec := range f.Sections {
			if addr >= sec.Addr && addr+n <= sec.Addr+sec.Size && sec.Type != elf.SHT_NOBITS {
				d, _ := sec.Data()
				fmt.Printf("%q\n", d[addr-sec.Addr:addr-sec.Addr+n])
			}
		}
		return
	}
	if os.Args[2] == "deref" {
		var addr uint64
		fmt.Sscanf(os.Args[3], "%x", &addr)
		for _, sec := range f.Sections {
			if addr >= sec.Addr && addr+8 <= sec.Addr+sec.Size && sec.Type != elf.SHT_NOBITS {
				d, _ := sec.Data()
				v := binary.LittleEndian.Uint64(d[addr-sec.Addr:])
				syms, _ := f.Symbols()
				for _, s := range syms {
					if v >= s.Value && v < s.Value+s.Size {
						fmt.Printf("%#x -> %#x %s\n", addr, v, s.Name)
					}
				}
			}
		}
		return
	}
	re := regexp.MustCompile(os.Args[2])
	syms, _ := f.Symbols()
	sort.Slice(syms, func(i, j int) bool { return syms[i].Name < syms[j].Name })
	for _, s := range syms {
		if !re.MatchString(s.Name) || s.Size == 0 {
			continue
		}
		sec := f.Sections[s.Section]
		if sec.Type == elf.SHT_NOBITS {
			fmt.Printf("%s size=%d (bss: zero)\n", s.Name, s.Size)
			continue
		}
		data, err := sec.Data()
		if err != nil {
			continue
		}
		off := s.Value - sec.Addr
		b := data[off : off+s.Size]
		fmt.Printf("%s size=%d sec=%s\n ", s.Name, s.Size, sec.Name)
		for i := 0; i+8 <= len(b) && i < 256; i += 8 {
			u := binary.LittleEndian.Uint64(b[i:])
			fmt.Printf(" [%d] %#x i=%d f=%g\n ", i, u, int64(u), math.Float64frombits(u))
		}
		if len(b) < 8 {
			fmt.Printf(" % x", b)
		}
		fmt.Println()
	}
}
