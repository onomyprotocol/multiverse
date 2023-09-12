package docs

import "embed"

//go:embed static
var Docs embed.FS

// NOTE: to regenerate openapi.yml, `rm -rf vendor`,
// use Ignite v0.23.0 (do not use later versions until we
// upgrade past .45) and `ignite generate openapi`
