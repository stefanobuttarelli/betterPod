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
	for _, path := range os.Args[1:] {
		f, err := parser.ParseFile(fset, path, nil, 0)
		if err != nil { fmt.Fprintln(os.Stderr, err); continue }
		base := filepath.Base(path)
		for _, d := range f.Decls {
			fd, ok := d.(*ast.FuncDecl)
			if !ok { continue }
			name := fd.Name.Name
			if fd.Recv != nil && len(fd.Recv.List) > 0 {
				t := fd.Recv.List[0].Type
				star := ""
				if s, ok := t.(*ast.StarExpr); ok { t = s.X; star = "*" }
				if id, ok := t.(*ast.Ident); ok {
					if star != "" { name = "(*" + id.Name + ")." + name } else { name = id.Name + "." + name }
				}
			}
			fmt.Printf("%s\tmain.%s\t%d\t%d\n", base, name, fset.Position(fd.Pos()).Line, fset.Position(fd.End()).Line)
		}
	}
}
