# Stack technique — décisions (validées le 26/11/2026)

| Couche | Choix | Pourquoi |
|---|---|---|
| Moteur | **Rust** (`crates/core`) | Rapide, sûr, compilateur strict ; compile en natif et en WebAssembly. |
| Pont web | **wasm-bindgen** + **ts-rs** | Les types TypeScript sont générés depuis Rust : interface et moteur ne peuvent pas diverger. |
| Interface | **Svelte 5 + TypeScript strict** | Concis, lisible, bien maîtrisé par les agents. |
| Composants | **shadcn-svelte** (Bits UI) + **Tailwind CSS 4** + **Lucide** | Composants accessibles et cohérents, thème clair/sombre. |
| Aperçu SVG | Le navigateur lui-même | Rendu vectoriel net à tout zoom, sans rien recoder. |
| Bureau | **Tauri 2** | Application légère (quelques Mo), dialogues de fichiers natifs. |
| Moteur dans Tauri | **Même WASM que le web** | Un seul chemin de code ; moteur natif multithread possible plus tard derrière la même interface. |
| Qualité | `crates/eval` (resvg + ΔE Oklab) | Score objectif pour faire progresser l'algorithme sans juger « à l'œil ». |
| Tests | cargo test, Vitest, Playwright | Tout s'exécute sans écran, en CI comme en local. |
| Licence | **MIT OR Apache-2.0** | Standard Rust, compatible avec VTracer et kurbo ; **aucun code GPL**. |

Écartés : Godot (pas de rendu SVG vectoriel, web lourd, Rust sur le web expérimental), egui (aperçu rastérisé,
rendu moins soigné), Dioxus/Leptos (jeunes, peu de composants), Electron (×10 en taille).

## Distribution
| Plateforme | Format | Remarque |
|---|---|---|
| Linux | **AppImage** (portable) | Un fichier, `chmod +x` puis lancer. |
| Windows | **`.exe` portable** (+ installateur NSIS) | Nécessite WebView2, présent sur Windows 10/11. Non signé : avertissement SmartScreen au 1er lancement. |
| macOS | `.dmg` universel | Priorité basse ; non signé ni notarisé (clic droit → Ouvrir). |
| Web | Site statique sur **GitHub Pages** | Fonctionne hors ligne une fois chargé (PWA prévue). |

Chaque plateforme est compilée sur son propre OS dans GitHub Actions, déclenché par un tag `vX.Y.Z`.
