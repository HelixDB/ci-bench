// Package render renders the embedded templates of package assets.
package render

import (
	"strings"

	"example.com/adv/assets"
)

// Catalog is the templates' names, comma-separated.
func Catalog() string {
	return strings.Join(assets.Names(), ",")
}
