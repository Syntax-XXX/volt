package main
import "fmt"
func main() { n := 5000000; f := make([]bool, n+1); for i := range f { f[i] = true }
 for i := 2; i*i <= n; i++ { if f[i] { for j := i*i; j <= n; j += i { f[j] = false } } }
 c := 0; for k := 2; k <= n; k++ { if f[k] { c++ } }; fmt.Println(c) }
