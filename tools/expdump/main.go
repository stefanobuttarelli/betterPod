package main

import (
	"fmt"
	"go/importer"
	"go/token"
	"go/types"
	"io"
	"os"
	"sort"
)

func main() {
	fset := token.NewFileSet()
	imp := importer.ForCompiler(fset, "gc", func(path string) (io.ReadCloser, error) {
		if path == "main" {
			return os.Open(os.Args[1])
		}
		// deps: resolve from cache via GOROOT/std export? use go list -export
		p, ok := deps[path]
		if !ok {
			return nil, fmt.Errorf("no dep %s", path)
		}
		return os.Open(p)
	})
	pkg, err := imp.(types.ImporterFrom).ImportFrom("main", "", 0)
	if err != nil {
		fmt.Fprintln(os.Stderr, "err:", err)
		if pkg == nil { os.Exit(1) }
	}
	sc := pkg.Scope()
	names := sc.Names()
	sort.Strings(names)
	q := func(p *types.Package) string { if p == pkg { return "" }; return p.Name() }
	for _, n := range names {
		o := sc.Lookup(n)
		if c, ok := o.(*types.Const); ok {
			fmt.Println(types.ObjectString(o, q), "=", c.Val().ExactString())
		} else {
			fmt.Println(types.ObjectString(o, q))
		}
		if tn, ok := o.(*types.TypeName); ok {
			if nt, ok := tn.Type().(*types.Named); ok {
				if st, ok := nt.Underlying().(*types.Struct); ok {
					for i := 0; i < st.NumFields(); i++ {
						f := st.Field(i)
						tag := st.Tag(i)
						if tag != "" { tag = " `" + tag + "`" }
						fmt.Printf("\t%s %s%s\n", f.Name(), types.TypeString(f.Type(), q), tag)
					}
				}
				for i := 0; i < nt.NumMethods(); i++ {
					fmt.Println("\tmethod", types.ObjectString(nt.Method(i), q))
				}
			}
		}
	}
}
