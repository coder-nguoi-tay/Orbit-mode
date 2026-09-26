# Lessons Learned

Padrões de erro identificados durante o desenvolvimento. Leia ao início de cada sessão.

---

## Quotas por conta

**Regra:** Ao receber um evento de quota de uma janela, preservar a última amostra da outra janela para a mesma conta e indexar o estado por `provider_account_id`.

**Por quê:** Eventos Codex de 5h e 7d chegam separadamente; substituir o objeto inteiro apaga a janela anterior e pode liberar uma conta ainda esgotada. A utilização é uma fração entre 0 e 1, então a UI precisa multiplicar por 100 para exibir porcentagem.

**Quando aplicar:** Em stores, snapshots, notificações e telas de uso com múltiplos perfis.

---

## GIF / Puppeteer

**Regra:** O parâmetro `delay` do gifenc está em **milissegundos** (não centissegundos como o spec GIF). Use `Math.round(1000 / FPS)`.
**Por quê:** Passando `100 / FPS` (centissegundos), o GIF ficava ~10× mais rápido que o esperado.
**Quando aplicar:** Sempre que usar gifenc para gerar GIFs animados.

---

**Regra:** Usar `.flatten({ background: '#ffffff' }).ensureAlpha()` no sharp antes de passar para `quantize`, e passar `{ clearAlphaColor: 255 }` ao quantize.
**Por quê:** `clearAlphaColor` padrão é `0` (preto) — pixels transparentes viravam preto, corrompendo frames com fundo escuro/transparente.
**Quando aplicar:** Ao converter screenshots para GIF com gifenc.

---

**Regra:** Usar `page.waitForSelector('.item')` antes da primeira captura, não apenas `sleep()`.
**Por quê:** `sleep` fixo não garante que o conteúdo renderizou — o primeiro frame saía branco.
**Quando aplicar:** Sempre que capturar screenshots com Puppeteer após navegação.

---

**Regra:** Após rodar o gerador de GIF, verificar se a porta 1420 ficou em TIME_WAIT antes de rodar novamente.
**Por quê:** Tentativas consecutivas falham com `ERR_CONNECTION_REFUSED` ou navigation timeout porque o Vite não consegue bindar a porta.
**Quando aplicar:** Ao rodar `npm run demo:gif` mais de uma vez seguida.

---

## Git / Versionamento

**Regra:** Ao fazer `git stash` + `git pull` + `git stash pop`, verificar se arquivos staged antes do stash foram incluídos no commit seguinte.
**Por quê:** A deleção do screenshot foi incluída no commit de bump de versão sem intenção, pois estava staged antes do stash.
**Quando aplicar:** Sempre que usar stash para contornar conflitos de pull.

---

## Verificacoes Pre-Commit

**Regra:** Rodar `cargo fmt --manifest-path tauri/Cargo.toml --check` junto com clippy antes de todo commit. Não confiar só no pre-commit hook.
**Por quê:** O cargo clippy compila mas não verifica formatação. O CI falha com `cargo fmt --check` mesmo com clippy passando localmente — como aconteceu no PR do Feed UI.
**Quando aplicar:** Sempre ao commitar código Rust.

---

## Execucao de Planos

**Regra:** Ao executar um plano com multiplos blocos, verificar no codigo que cada bloco visivel foi conectado ao fluxo real da UI antes de declarar concluido.
**Por quê:** Implementar o Git panel sem conectar o bloco de tabs/header deixou os ajustes visuais aprovados invisiveis no app.
**Quando aplicar:** Sempre que um plano incluir componentes novos e wiring em stores/containers, especialmente planos com etapas sobrepostas.

---

## rtk git bypassa hooks

**Regra:** `rtk git commit` pode bypassar o pre-commit hook. Sempre rodar `cargo fmt --manifest-path tauri/Cargo.toml` manualmente antes de qualquer commit de código Rust. Idem para `cargo clippy`, `npx eslint`, `npx prettier`.

**Por quê:** O CI roda `cargo fmt --check` e falha se a formatação não estiver correta. O pre-commit hook roda auto-fix mas se o commit for feito via `rtk` o hook pode ser ignorado.

**Quando aplicar:** Todo commit que tocar código Rust.

---

## Criacao de Branch com Tracking Automatico

**Regra:** Ao criar uma nova branch a partir de um remote (`git checkout -b <nome> <remote>/<branch>`), SEMPRE usar `--no-track` para evitar tracking automático. Ou crie a branch primeiro, depois faça rebase/merge manual.

**Exemplo correto:**
```bash
git checkout -b fix/chat-feed --no-track origin/dev
```

**Por quê:** `git checkout -b fix/chat-feed origin/dev` sem `--no-track` configurou tracking automático para `origin/dev`. O `git branch -vv` mostrava `[origin/dev]`, dando a impressão incorreta de que commits iriam direto para `dev`. Apesar de na prática `git push` sem `-u` não funcionar, a configuração de tracking causou confusão sobre o comportamento esperado.

**Quando aplicar:** Sempre ao criar branches a partir de um remote. Verificar com `git branch -vv` após a criação.

---

## OpenCode Providers

**Regra:** Não usar o `opencode.json` do diretório do projeto Orbit como fonte de providers do OpenCode. A lista deve vir de `~/.cache/opencode/models.json` e da config global do próprio OpenCode em `~/.config/opencode/opencode.json` / `.jsonc`; deixe o CLI validar providers default e autenticação no spawn.

**Por quê:** O `opencode.json` na raiz do Orbit é usado para MCP/plugins do projeto e não contém necessariamente blocos `provider`. Usá-lo como config de provider gera falsos erros como pedir `provider.crof` em `C:\Users\...\orbit/opencode.json` e marca providers default como não configurados.

**Quando aplicar:** Sempre que mexer em listagem, normalização ou validação de providers/modelos OpenCode.

## File Explorer Projects

**Quy tắc:** Không đặt giới hạn số file hoặc độ sâu tùy ý khi quét cây project để hiển thị; chỉ loại trừ các thư mục dependency/cache đã xác định rõ.

**Vì sao:** Một thư mục lớn như `core` có thể vượt giới hạn trước khi bộ quét tới các thư mục và file ở root như `database`, `docker`, `composer.lock` hoặc `phpunit.xml`, làm Explorer hiển thị thiếu nhưng không báo lỗi.

**Khi áp dụng:** Khi sửa API `list_project_files` hoặc UI Explorer của Orbit.

---

## Terminal Split Rendering

**Quy tắc:** Khi chuyển workspace từ một pane sang hai pane, giữ nguyên instance của pane đang hiển thị và chỉ mount pane mới; loading overlay phải nằm ngoài luồng flex hoặc phủ tuyệt đối toàn bộ panel.

**Vì sao:** Thay cả nhánh `PaneContainer` bằng `SplitContainer` làm chat dài bị unmount/mount lại trước frame đầu tiên, còn overlay và terminal cùng `flex: 1` khiến trạng thái loading chỉ rộng nửa panel.

**Khi áp dụng:** Khi mở terminal, file editor hoặc utility pane dạng split từ một chat đang hoạt động.

---

## SvelteKit Build Trong Phiên Tauri Dev

**Quy tắc:** Không chạy `npm run build` đồng thời với `npm run tauri:dev`; dừng hoặc khởi động lại phiên Tauri sau khi production build ghi vào `.svelte-kit`.

**Vì sao:** WebView có thể giữ module graph của dev server trong lúc build thay generated runtime, tạo vòng import trộn phiên bản và lỗi TDZ như `Cannot access 'updated_listener' before initialization`.

**Khi áp dụng:** Khi xác minh production build trong lúc cửa sổ Orbit development đang mở.
