// Package assets embeds the templates under templates/.
package assets

import (
	"embed"
	"io/fs"
)

//go:embed templates/*.tmpl
var templates embed.FS

// Names lists the embedded templates, sorted.
func Names() []string {
	entries, err := fs.ReadDir(templates, "templates")
	if err != nil {
		panic(err)
	}
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		names = append(names, e.Name())
	}
	return names
}
