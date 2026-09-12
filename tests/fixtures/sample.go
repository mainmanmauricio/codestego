package main
import "fmt"
// Go sample with backticks
func main() {
    /* block note here */
    s := `line // not comment
/* still string */`
    fmt.Println(s)
}
