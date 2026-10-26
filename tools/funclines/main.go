// funclines prints "file<TAB>startline<TAB>endline<TAB>funcname" for Go sources
// (same naming as the compiler: main.(*T).m, main.f).
package main

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
)

func main() {
	fset := token.NewFileSet()
	for _, p := range os.Args[1:] {
		f, err := parser.ParseFile(fset, p, nil, 0)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			continue
		}
		for _, d := range f.Decls {
			fd, ok := d.(*ast.FuncDecl)
			if !ok {
				continue
			}
			name := "main." + fd.Name.Name
			if fd.Recv != nil && len(fd.Recv.List) > 0 {
				t := fd.Recv.List[0].Type
				if s, ok := t.(*ast.StarExpr); ok {
					name = fmt.Sprintf("main.(*%s).%s", s.X.(*ast.Ident).Name, fd.Name.Name)
				} else {
					name = fmt.Sprintf("main.%s.%s", t.(*ast.Ident).Name, fd.Name.Name)
				}
			}
			fmt.Printf("%s\t%d\t%d\t%s\n", filepath.Base(p), fset.Position(fd.Pos()).Line, fset.Position(fd.End()).Line, name)
		}
	}
}
