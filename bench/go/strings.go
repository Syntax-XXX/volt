package main
import "fmt"
func main() { total := 0; for i := 0; i < 500000; i++ { s := fmt.Sprintf("Eintrag %d von %d", i, i*2); total += len(s) }; fmt.Println(total) }
