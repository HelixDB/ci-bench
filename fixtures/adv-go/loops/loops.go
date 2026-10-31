// Package loops depends on the language version go.mod declares: from go 1.22 on, each loop
// iteration has its own variable.
package loops

// Captured is what closures created in a loop return.
func Captured() []int {
	var fs []func() int
	for i := 0; i < 3; i++ {
		fs = append(fs, func() int { return i })
	}
	out := make([]int, 0, len(fs))
	for _, f := range fs {
		out = append(out, f())
	}
	return out
}
