# File Editor

## Objetivo
Permitir abrir e editar arquivos de projetos diretamente na interface do Orbit, sem sair do app.

## Comportamento esperado
- Nova aba **"Files"** disponível no menu `+` da TabBar
- Abre vinculado ao `cwd` da sessão ativa no painel (ou ao cwd da aba git/terminal)
- Painel dividido: árvore de arquivos à esquerda, editor Monaco à direita
- Campo de busca filtra arquivos em tempo real (usa `search_project_files`)
- Clique em arquivo → abre no editor Monaco (lê via `read_file_content`)
- Edição direta no editor; Cmd/Ctrl+S salva (chama `write_file_content`)
- Indicador de alterações não salvas (•) no header da aba
- Confirmação ao trocar de arquivo com alterações não salvas

## Casos de borda
- Arquivo binário: exibe mensagem "Binary file - not editable"
- Arquivo muito grande (>500KB): exibe aviso e abre em modo somente leitura
- Erro ao salvar: toast de erro
- CWD sem arquivos: estado vazio com mensagem

## Critérios de aceitação
- [ ] Aba "Files" aparece no menu + da TabBar com ícone de pasta
- [ ] Lista de arquivos filtra conforme digitação
- [ ] Arquivo abre no editor Monaco com sintaxe destacada pela extensão
- [ ] Cmd/Ctrl+S salva e remove indicador de alteração
- [ ] Editor respeita o tema dark do Orbit
