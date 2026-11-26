# OpenVectorizer

Convertissez vos images (PNG, JPG, WebP, GIF, BMP) en **SVG propres**, **hors ligne** : rien n'est envoyé sur
un serveur. Disponible en application web et en application de bureau portable (Linux, Windows, macOS).

> 🚧 En développement actif. Le moteur actuel s'appuie sur VTracer et sera remplacé étape par étape
> (voir [la feuille de route](docs/ROADMAP.md)).

## Utiliser
- **Web** : https://de-seingalt.github.io/OpenVectorizer/
- **Bureau** : téléchargez la dernière version dans [Releases](https://github.com/de-seingalt/OpenVectorizer/releases)
  - Linux : `OpenVectorizer_*.AppImage` → `chmod +x` puis double-clic
  - Windows : `OpenVectorizer_*_portable.exe` → double-clic, aucune installation
  - macOS : `.dmg`
- **Ligne de commande** : `openvectorizer convert logo.png -o logo.svg`

## Développer
Prérequis : Rust (stable), Node 22+ avec pnpm (`corepack enable pnpm`), et
`cargo install wasm-bindgen-cli --version 0.2.130 --locked`.
Pour l'application de bureau sous Linux (Ubuntu/Debian) :
```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

```sh
cd app && pnpm install
pnpm dev                 # interface web : http://localhost:5173
pnpm tauri dev           # application de bureau
cd .. && scripts/check.sh   # toutes les vérifications de la CI
```

Architecture et conventions : [CLAUDE.md](CLAUDE.md) · Choix techniques : [docs/STACK.md](docs/STACK.md)

## Licence
Au choix : [MIT](LICENSE-MIT) ou [Apache-2.0](LICENSE-APACHE).
