// Cross-platform pre-build hook for `pnpm build` / `pnpm dev`: builds the WASM engine.
// CI builds WASM in a dedicated step and sets OV_SKIP_WASM=1.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

if (process.env.OV_SKIP_WASM) process.exit(0);
const script = fileURLToPath(new URL('./build-wasm.sh', import.meta.url));
const r = spawnSync('bash', [script, ...process.argv.slice(2)], { stdio: 'inherit' });
process.exit(r.status ?? 1);
