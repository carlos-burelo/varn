package main

import (
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"time"
)

type Node struct{ id int }

var sink int64

func w1() int {
	t := 0
	for i := 0; i < 300; i++ {
		a := make([]int64, 500000)
		for j := range a {
			a[j] = int64(i)
		}
		sink += a[7]
		t += len(a)
	}
	return t
}
func w3() int {
	keep := make([]*Node, 0)
	for i := 0; i < 4000000; i++ {
		keep = append(keep, &Node{i})
	}
	var ring [1024]*Node
	for i := range ring {
		ring[i] = &Node{0}
	}
	for i := 0; i < 4000000; i++ {
		ring[i&1023] = &Node{i}
	}
	return len(keep) + ring[5].id
}
func w5() int {
	size := 500000
	win := make([]*Node, 0, size)
	for i := 0; i < size; i++ {
		win = append(win, &Node{i})
	}
	s := 0
	for i := 0; i < 6000000; i++ {
		win[i%size] = &Node{i}
		s += win[(i+7)%size].id
	}
	return s
}
func main() {
	t0 := time.Now()
	var r int
	switch os.Args[1] {
	case "w1":
		r = w1()
	case "w3":
		r = w3()
	case "w5":
		r = w5()
	}
	wall := time.Since(t0).Milliseconds()
	var m runtime.MemStats
	runtime.ReadMemStats(&m)
	var mx uint64
	n := int(m.NumGC)
	if n > 256 {
		n = 256
	}
	for i := 0; i < n; i++ {
		if m.PauseNs[i] > mx {
			mx = m.PauseNs[i]
		}
	}
	out, _ := json.Marshal(map[string]any{"wall_ms": wall, "gcs": m.NumGC, "pause_total_ms": float64(m.PauseTotalNs) / 1e6, "pause_max_ms": float64(mx) / 1e6, "result": r + int(sink&1)})
	fmt.Println(string(out))
}
