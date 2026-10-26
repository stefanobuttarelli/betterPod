package main

// ij_parallel.go — split per-row rendering work over the CPU cores.

import (
	"runtime"
	"sync"
)

var numWorkers = func() int {
	n := runtime.NumCPU()
	if n > 4 {
		n = 4
	}
	if n < 1 {
		n = 1
	}
	return n
}()

// parallelRows runs fn over [y0, y1) split in bands, one per worker.
func parallelRows(y0, y1 int, fn func(int, int)) {
	n := y1 - y0
	if n <= 0 {
		return
	}
	w := numWorkers
	if n < 32 || w == 1 {
		fn(y0, y1)
		return
	}
	var wg sync.WaitGroup
	step := (n + w - 1) / w
	for a := y0; a < y1; a += step {
		b := a + step
		if b > y1 {
			b = y1
		}
		wg.Add(1)
		go func(a, b int) {
			defer wg.Done()
			fn(a, b)
		}(a, b)
	}
	wg.Wait()
}
