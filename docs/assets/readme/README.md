# README visual assets

This directory contains source-controlled presentation assets, not engineering evidence or runtime output.

| Asset | Purpose | Source |
| --- | --- | --- |
| `puppyone-logo.png` | Company identity | puppyone GitHub organization avatar, https://avatars.githubusercontent.com/u/143623203?v=4 |
| `hero.png` | Branded conceptual illustration | AI-generated from the official orange puppy mark; depicts databases, semantic context, and a governed agent gateway |
| `overview.svg` | Product architecture and the separate data/context paths | Original hand-drawn-style vector diagram; animated packets |
| `ontology-model.svg` | Data, semantic, and access planes | Original hand-drawn-style vector diagram |
| `self-build.svg` | Eleven S0–S10 stages and the review/classification boundary | Original hand-drawn-style vector diagram; animated downstream flow |
| `query-lifecycle.svg` | Admission, durable audit, read-only execution, result, and cleanup | Original hand-drawn-style vector diagram; animated request/result paths |
| `deployment.svg` | Intended network/deployment zones | Original hand-drawn-style vector diagram |

The brand palette is derived from the company mark: amber/orange and charcoal. Warm paper, muted mint, and pale blue distinguish diagram planes. The supplementary colors are presentation choices, not a newly declared corporate brand standard.

SVGs are self-contained, use no JavaScript, and require no external fonts or image resources. The animated SVGs use standard declarative SVG animation. They retain a complete static diagram when animation is unavailable, and their animated packet elements are hidden under `prefers-reduced-motion`. Host font availability can affect the handwriting font fallback; all diagram text remains native SVG text with accessible titles/descriptions.

All README image paths are repository-relative. Images and source SVGs are stored together so GitHub can serve them directly. Architecture and deployment diagrams follow the product documentation.
