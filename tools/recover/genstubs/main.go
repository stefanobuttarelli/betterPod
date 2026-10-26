// genstubs writes zz_stubs.go: declarations of everything the v0.20 build had
// (types, globals, functions, methods) that the current sources don't declare
// yet, with types taken from the DWARF of the relinked v0.20 object.
// usage: genstubs dbg.elf srcdir > srcdir/zz_stubs.go
package main

import (
	"debug/dwarf"
	"debug/elf"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

var pkgRe = regexp.MustCompile(`([a-zA-Z0-9_\-./]+/)?([a-z][a-z0-9_]*)\.([A-Za-z_]\w*)`)
var imports = map[string]bool{}

func goType(s string) string {
	s = strings.TrimSpace(s)
	if strings.HasPrefix(s, "struct ") && !strings.HasPrefix(s, "struct {") {
		s = strings.TrimPrefix(s, "struct ")
	}
	for _, r := range [][2]string{{"struct []", "[]"}, {"struct string", "string"}, {"struct struct {", "struct {"}} {
		s = strings.ReplaceAll(s, r[0], r[1])
	}
	// anonymous struct: "struct { main.a *main.Anim; main.until time.Time }"
	s = strings.ReplaceAll(s, "main.", "")
	s = pkgRe.ReplaceAllStringFunc(s, func(m string) string {
		sm := pkgRe.FindStringSubmatch(m)
		path := sm[1] + sm[2]
		switch sm[2] {
		case "unsafe":
			imports["unsafe"] = true
		default:
			imports[path] = true
		}
		return sm[2] + "." + sm[3]
	})
	return s
}

type fn struct {
	name, recv, recvType string
	params, results []string
}

func main() {
	f, err := elf.Open(os.Args[1])
	if err != nil {
		panic(err)
	}
	d, _ := f.DWARF()
	declared := map[string]bool{}
	expTypes := map[string]string{}
	expConsts := map[string]string{}
	expFuncs := map[string]string{}
	expMethods := map[string][2]string{}
	if data, err := os.ReadFile(os.Getenv("EXPORTS")); err == nil {
		for _, l := range strings.Split(string(data), "\n") {
			if strings.HasPrefix(l, "type ") {
				n := strings.Fields(l)[1]
				expTypes[n] = l
			}
			if m := regexp.MustCompile(`^\s*method func \((\*?)(\w+)\)\.(\w+)(\(.*)$`).FindStringSubmatch(l); m != nil {
				expMethods[m[2]+"."+m[3]] = [2]string{m[1] + m[2], m[3] + m[4]}
			}
			if strings.HasPrefix(l, "func ") {
				n := strings.Fields(l)[1]
				if i := strings.Index(n, "("); i > 0 {
					n = n[:i]
				}
				expFuncs[n] = l
			}
			if strings.HasPrefix(l, "const ") {
				n := strings.Fields(l)[1]
				expConsts[n] = l
			}
		}
	}
	fset := token.NewFileSet()
	files, _ := filepath.Glob(filepath.Join(os.Args[2], "*.go"))
	for _, p := range files {
		if strings.HasSuffix(p, "zz_stubs.go") {
			continue
		}
		af, err := parser.ParseFile(fset, p, nil, 0)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			continue
		}
		for _, dcl := range af.Decls {
			switch x := dcl.(type) {
			case *ast.FuncDecl:
				n := x.Name.Name
				if x.Recv != nil && len(x.Recv.List) > 0 {
					t := x.Recv.List[0].Type
					if s, ok := t.(*ast.StarExpr); ok {
						t = s.X
					}
					if id, ok := t.(*ast.Ident); ok {
						n = id.Name + "." + n
					}
				}
				declared["f:"+n] = true
			case *ast.GenDecl:
				for _, sp := range x.Specs {
					switch s := sp.(type) {
					case *ast.TypeSpec:
						declared["t:"+s.Name.Name] = true
					case *ast.ValueSpec:
						for _, nm := range s.Names {
							declared["v:"+nm.Name] = true
						}
					}
				}
			}
		}
	}
	var out []string
	for n, l := range expConsts {
		if !declared["v:"+n] {
			// "const A Key = 32" / "const X untyped int = 5"
			l = strings.Replace(l, " untyped int ", " ", 1)
			l = strings.Replace(l, " untyped float ", " ", 1)
			l = strings.Replace(l, " untyped string ", " ", 1)
			l = strings.Replace(l, " untyped rune ", " ", 1)
			l = strings.Replace(l, " untyped bool ", " ", 1)
			out = append(out, l+"\n")
		}
	}
	useExp := func(n string) bool {
		if l, ok := expTypes[n]; ok {
			out = append(out, goType(l)+"\n")
			return true
		}
		return false
	}
	r := d.Reader()
	typeOf := func(e *dwarf.Entry) string {
		off, ok := e.Val(dwarf.AttrType).(dwarf.Offset)
		if !ok {
			return ""
		}
		t, err := d.Type(off)
		if err != nil {
			return "any"
		}
		return t.String()
	}
	var fns []fn
	seenT := map[string]bool{}
	seenF := map[string]bool{}
	for {
		e, err := r.Next()
		if err != nil || e == nil {
			break
		}
		name, _ := e.Val(dwarf.AttrName).(string)
		switch e.Tag {
		case dwarf.TagSubprogram:
			if !strings.HasPrefix(name, "main.") || (e.Val(dwarf.AttrLowpc) == nil && e.Val(dwarf.AttrInline) == nil) {
				r.SkipChildren()
				continue
			}
			n := strings.TrimPrefix(name, "main.")
			var x fn
			if strings.Contains(n, ".func") || strings.HasSuffix(n, "-fm") || strings.ContainsAny(n, "·") || strings.HasPrefix(n, "map.") || strings.Contains(n, "wrap") || strings.HasPrefix(n, "_C") || strings.HasPrefix(n, "_cgo") || strings.HasPrefix(n, "init") || n == "main" {
				r.SkipChildren()
				continue
			}
			if m := regexp.MustCompile(`^\(\*(\w+)\)\.(\w+)$`).FindStringSubmatch(n); m != nil {
				x.recvType, x.name = "*"+m[1], m[2]
			} else if m := regexp.MustCompile(`^(\w+)\.(\w+)$`).FindStringSubmatch(n); m != nil {
				x.recvType, x.name = m[1], m[2]
			} else {
				x.name = n
			}
			key := x.name
			if x.recvType != "" {
				key = strings.TrimPrefix(x.recvType, "*") + "." + x.name
			}
			if declared["f:"+key] || seenF[key] {
				r.SkipChildren()
				continue
			}
			seenF[key] = true
			if e.Val(dwarf.AttrLowpc) == nil {
				// inline-only: the DWARF has no results; use the export data or skip
				if l, ok := expFuncs[x.name]; ok && x.recvType == "" {
					out = append(out, goType(l)+" { panic(\"stub\") }\n")
				}
				if m, ok := expMethods[key]; ok && x.recvType != "" {
					// method signature from the export data; parameter names kept
					out = append(out, goType(fmt.Sprintf("func (_ %s) %s", m[0], m[1]))+" { panic(\"stub\") }\n")
				}
				r.SkipChildren()
				continue
			}
			first := true
			if e.Children {
				for {
					c, _ := r.Next()
					if c == nil || c.Tag == 0 {
						break
					}
					if c.Tag == dwarf.TagFormalParameter {
						pn, _ := c.Val(dwarf.AttrName).(string)
						ty := goType(typeOf(c))
						if x.recvType != "" && first {
							x.recv = pn
							first = false
							continue
						}
						first = false
						dup := false
						for _, q := range x.params {
							if q == pn+" "+ty {
								dup = true
							}
						}
						if strings.HasPrefix(pn, "~r") {
							if len(x.results) > 0 && x.results[len(x.results)-1] == ty && pn == "~r0" {
								dup = true
							}
						}
						if dup {
							if c.Children {
								r.SkipChildren()
							}
							continue
						}
						if strings.HasPrefix(pn, "~r") {
							x.results = append(x.results, ty)
						} else {
							if pn == "" || pn == "_" {
								pn = "_"
							}
							x.params = append(x.params, pn+" "+ty)
						}
					}
					if c.Children {
						r.SkipChildren()
					}
				}
			}
			fns = append(fns, x)
		case dwarf.TagStructType:
			if strings.HasPrefix(name, "main.") && !strings.ContainsAny(name, "[]*{ ·") && !strings.HasPrefix(name, "main._C") {
				n := strings.TrimPrefix(name, "main.")
				if !declared["t:"+n] && !seenT[n] {
					seenT[n] = true
					if useExp(n) {
						r.SkipChildren()
						continue
					}
					t, err := d.Type(e.Offset)
					if err == nil {
						st := t.(*dwarf.StructType)
						var b strings.Builder
						fmt.Fprintf(&b, "type %s struct {\n", n)
						for _, fl := range st.Field {
							fmt.Fprintf(&b, "\t%s %s\n", fl.Name, goType(fl.Type.String()))
						}
						b.WriteString("}\n")
						out = append(out, b.String())
					}
				}
			}
			r.SkipChildren()
		case dwarf.TagTypedef:
			if strings.HasPrefix(name, "main.") && !strings.ContainsAny(name, "[]*{ ·") && !strings.HasPrefix(name, "main._C") {
				n := strings.TrimPrefix(name, "main.")
				if !declared["t:"+n] && !seenT[n] {
					seenT[n] = true
					if useExp(n) {
						r.SkipChildren()
						continue
					}
					ut := typeOf(e)
					if strings.Contains(ut, "runtime.iface") || strings.Contains(ut, "runtime.eface") {
						out = append(out, fmt.Sprintf("type %s interface{} // TODO methods\n", n))
					} else {
						out = append(out, fmt.Sprintf("type %s %s\n", n, goType(ut)))
					}
				}
			}
			r.SkipChildren()
		case dwarf.TagVariable:
			if strings.HasPrefix(name, "main.") {
				n := strings.TrimPrefix(name, "main.")
				if !declared["v:"+n] && !strings.ContainsAny(n, ".·") && !strings.HasPrefix(n, "_C") && !strings.HasPrefix(n, "_cgo") {
					out = append(out, fmt.Sprintf("var %s %s\n", n, goType(typeOf(e))))
				}
			}
			r.SkipChildren()
		case dwarf.TagBaseType:
			if strings.HasPrefix(name, "main.") && !strings.ContainsAny(name, "[]*{ ·") && !strings.HasPrefix(name, "main._C") {
				n := strings.TrimPrefix(name, "main.")
				if !declared["t:"+n] && !seenT[n] {
					seenT[n] = true
					if !useExp(n) {
						out = append(out, fmt.Sprintf("type %s int // TODO kind\n", n))
					}
				}
			}
		case dwarf.TagCompileUnit:
		default:
			if e.Children {
				r.SkipChildren()
			}
		}
	}
	// functions the v0.20 compiler never inlined get //go:noinline, so a
	// panicking stub doesn't get inlined and turn the caller's code dead
	inlined := map[string]bool{}
	if data, err := os.ReadFile("/home/claude/recover/dwarf.txt"); err == nil {
		for _, l := range strings.Split(string(data), "\n") {
			l = strings.TrimSpace(l)
			if strings.HasPrefix(l, "inline main.") {
				n := strings.Fields(l)[1]
				inlined[strings.TrimPrefix(n, "main.")] = true
			}
		}
	}
	for _, x := range fns {
		var b strings.Builder
		key := x.name
		if x.recvType != "" {
			key = "(" + x.recvType + ")." + x.name
		}
		if !inlined[key] {
			b.WriteString("//go:noinline\n")
		}
		b.WriteString("func ")
		if x.recvType != "" {
			rn := x.recv
			if rn == "" {
				rn = "_"
			}
			fmt.Fprintf(&b, "(%s %s) ", rn, x.recvType)
		}
		fmt.Fprintf(&b, "%s(%s)", x.name, strings.Join(x.params, ", "))
		if len(x.results) == 1 {
			b.WriteString(" " + x.results[0])
		} else if len(x.results) > 1 {
			b.WriteString(" (" + strings.Join(x.results, ", ") + ")")
		}
		b.WriteString(" { panic(\"stub\") }\n")
		out = append(out, b.String())
	}
	var imps []string
	for p := range imports {
		imps = append(imps, p)
	}
	sort.Strings(imps)
	fmt.Println("// Code generated by genstubs; DO NOT EDIT. Missing v0.20 declarations.\n\npackage main\n")
	if len(imps) > 0 {
		fmt.Println("import (")
		for _, p := range imps {
			fmt.Printf("\t%q\n", p)
		}
		fmt.Println(")\n")
	}
	for _, s := range out {
		fmt.Print(s)
	}
}
