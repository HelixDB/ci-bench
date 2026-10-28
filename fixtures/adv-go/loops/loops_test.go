package loops

import (
	"reflect"
	"testing"
)

func TestCaptured(t *testing.T) {
	if got, want := Captured(), []int{0, 1, 2}; !reflect.DeepEqual(got, want) {
		t.Fatalf("Captured() = %v, want %v", got, want)
	}
}
