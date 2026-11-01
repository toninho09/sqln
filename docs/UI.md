# SQLN — Esboços das telas

## Estado do desenho

Este documento apresenta uma proposta visual baseada no [plano funcional](../PLAN.md).
O plano continua sendo a referência das decisões confirmadas.

Os valores, nomes e dados abaixo são exemplos fictícios.
Tamanhos, espaçamentos e cores ainda são propostas visuais.
Os atalhos aprovados estão em `docs/KEYMAP.md`.
O usuário aprova os fluxos e ajustes registrados em `docs/DECISIONS.md` em 2026-10-31.
Essa aprovação não significa que todos os fluxos já existem no código.
A interface final usa inglês; os rótulos em português destes esboços são exemplos.
A aplicação implementa as seções 1 a 4 e 6, e parte das seções 5 e 8.
O cadastro oferece **Session only** e **System vault**. Sem cofre, a gravação oferece texto simples depois do aviso.
A tela de abertura da seção 1 é uma lista sobre a tela, e não um painel com busca.
A seção 2 descreve a tela atual, no estilo aprovado em 2026-10-31.
Os outros esboços mostram o conteúdo de cada tela, e não o seu desenho exato.
As janelas têm borda arredondada, e os botões perdem os colchetes nos temas com cor.

## 1. Inicialização e abertura de console

Se não encontrar `.sqln/` no diretório atual ou nos superiores, perguntar sobre sua criação.
Aceitar cria o projeto no diretório atual. Recusar mantém somente o escopo global.

```text
┌─ Criar projeto ─────────────────────────────────────┐
│ Nenhuma pasta .sqln/ foi encontrada.                 │
│ Criar um projeto no diretório atual?                │
│                                                    │
│ [Criar projeto]  [Usar somente configurações globais]│
└────────────────────────────────────────────────────┘
```

Ao iniciar, a aplicação reabre as abas da última sessão, na mesma ordem e com a mesma aba ativa.
A opção **Reopen tabs** do diálogo Settings (`reopenTabs` em `config.json`) desliga isso. O padrão é ligado.

`sqln arquivo.db` e `sqln postgres://usuario@host/banco` abrem uma conexão só para esta sessão.
Nesse caso, as abas salvas não abrem, a pergunta **Create project** não aparece e nada vai para os arquivos.
O projeto usa a lista do seu `workspace.json`. Sem projeto, a aplicação usa a lista global.
Nenhuma aba executa SQL ou abre uma conexão ao iniciar.
Um console sem texto não volta, porque não tem arquivo. Uma aba cuja conexão saiu do cadastro também não volta.
Um console que outra instância usa fica fechado, e a lista do arquivo não muda ao sair.

Implementação atual: `n` no navegador abre uma lista sobre a tela, com o título `Consoles of <conexão> · <escopo>`.
A primeira linha, **+ New console**, cria um console. As outras linhas mostram o nome, a idade da última alteração e a marca `(open)`.
A lista não tem campo de busca nem botões. `Enter` escolhe, `Delete` remove, e `Esc` cancela.
`s` no navegador abre a lista **Scripts without a connection**, que mostra também o escopo de cada script.

```text
┌─ Conexões ──────────┬─ Consoles de app (local) ──────────────┐
│ app (local)        │ Buscar: [                             ]│
│ app (global)       │                                        │
│ warehouse          │ console-1                hoje, 10:30    │
│                    │ investigar-pedidos       ontem, 16:20   │
│                    │                                        │
│                    │ [Abrir]  [Criar novo console]           │
└────────────────────┴────────────────────────────────────────┘
```

O campo de busca e a apresentação de horários são propostas de desenho.
A filtragem por conexão e a oferta de reutilização já estão definidas.

Abrir um console já aberto seleciona sua aba existente.
Criar outro console gera um nome automático, que pode ser alterado depois.
`Ctrl+W N` renomeia o console atual; a lista também oferece a ação.
Recusar nomes vazios ou repetidos na mesma conexão, sem mudar identidade, arquivos, conexão ou escopo.

`Ctrl+W a` abre a troca ou associação de conexão do console.
Bloquear durante execução ou transação aberta.
Mostrar a conexão atual, a nova conexão e a pasta de destino antes da confirmação.
Salvar antes da troca; uma falha cancela a troca.
Preservar texto, nome, cursor e rolagem; limpar resultados, mensagens e seleção de banco ou schema.
Não executar nem converter o SQL automaticamente.

## 2. Visão de console

Editor e resultados aparecem juntos.
O usuário pode arrastar o divisor e recolher o navegador de conexões.

```text
▎ ◐ 12.345s  console-1 ×   ✓  orders ×   ✗  users (table) ×
──────────────────────────────────────────────────────────────────────
 EXPLORER        │ app › public › console-1
 ▾ ● ⛁ app  pg   │ ▶ Run  ▶▶ Script  ■ Stop  ◐ Auto  ✓ Commit  ↩ Rollback  ◇ public
   ▾ ◇ public    │  1 ✓ SELECT id, name                               ▐
     ▸ ▦ users   │  2   FROM public.users;                            │
     ▸ ◫ v_sales │  3                                                 │
                 │─ Result 1 ─ Result 2 ─ Messages ─────────────────────
                 │    │ id │ name   │ active
                 │  1 │  1 │ Ana    │ true                             ▐
                 │  2 │  2 │ Bruno  │ true                             │
                 │ 1,000 rows · row cap reached
 EDITOR  app  public  Auto · No transaction · Idle  Pool 0/5 · Data 1/256 MiB  3:14
 F5 run  Shift+F5 script  Ctrl+Space suggestions  Ctrl+F find  F1 keys
```

Com `"preferences": { "icons": "nerd" }` em `config.json`, o logo do banco fica antes do nome, e `pg` sai da linha.
Esse conjunto muda todos os símbolos da interface e exige uma Nerd Font.
No iTerm, instale o Nerd Font Bundle em `Settings > Profiles > Text > Manage Special Exceptions`. O texto continua com a fonte do perfil.

A linha sob as abas e o título do painel aparecem somente com 30 linhas ou mais.
Em 80 x 24, o editor e a grade dividem 18 linhas.

**Estado da execução nas abas:** um selo colorido fica no início da aba.

| Selo | Estado | Sem cor (`NO_COLOR`) |
|---|---|---|
| `◐ 12.345s` | Executando, com o tempo. A marca gira | `\| 12.345s` |
| `✓` | Terminou | `ok` |
| `✗` | Erro, tempo limite ou conexão perdida | `ERR` |
| `■` | Parada pelo usuário | `stop` |

O selo do resultado fica até o usuário sair da aba em que o viu. Uma aba que termina fora da tela mantém o selo até uma visita.
A próxima execução troca o selo. Consoles e tabelas usam os mesmos selos.

**Tempo:** a barra de status mostra `Running 12.345s` durante a execução e `Idle (last 2.440s)` depois.
O tempo tem três casas decimais, e a tela atualiza a cada 100 ms somente durante uma execução. Tempos longos usam `1m 05.123s` e `1h 02m 05.123s`.
O tempo começa com a execução, então a espera por senha não conta.

**Abas:** cada aba tem `×` no fim. `NO_COLOR` mostra `X`, e Nerd Font mostra o seu símbolo.
Um clique em `×` ou na coluna depois dele fecha a aba, com as perguntas de `Ctrl+W c`.
Fechar outra aba não muda a aba na tela. Fechar a última aba encerra a aplicação.
Uma aba que a linha corta não mostra `×`. Um clique nela abre a aba inteira.

**Linha de dicas:** cada tecla usa o estilo das teclas do menu do `Ctrl+W`, e o efeito usa a cor de dica.
Dois espaços separam os comandos. Sem cor, a tecla fica em negrito e o efeito fica fraco.
A linha lista só os comandos que agem no estado da tela, do mais relevante para o menos relevante.
Um comando que não cabe sai inteiro da linha, então a tela nunca mostra uma tecla cortada. `F1` lista todas as teclas.

| Estado | Primeiros comandos |
|---|---|
| Editor | Encerrar a transação, executar, ir para os resultados e sugestões |
| Editor sem conexão | Escolher a conexão e ir para o navegador |
| Editor em execução | Parar |
| Resultados | Parar, encerrar a transação, voltar ao editor, copiar, exportar e abrir o valor |
| **Messages** | Voltar ao editor e trocar de aba |
| Grade de uma tabela | Parar, carregar, `WHERE`, `ORDER BY`, ordenar, mais linhas e página ou seção |
| Linha de conexão | Abrir, expandir, estrutura, novo console, atualizar, editar, desconectar, senha, remover e adicionar |
| Linha de schema | Expandir, estrutura, novo console e atualizar |
| Linha de tabela ou view | Abrir, colunas, estrutura, novo console e atualizar |
| Linha de coluna | Voltar à tabela, estrutura, novo console e atualizar |

- `commit` e `rollback` aparecem só com uma transação aberta. Depois de um erro na transação, aparece só `rollback`.
- `disc` aparece só com uma sessão aberta. `pwd` aparece só em uma conexão que usa senha.
- Copiar, abrir o valor e buscar aparecem só com linhas na grade.
- `more` aparece só com mais linhas para ler. `page` aparece só com mais de uma página.
- Nos formulários, a linha muda com o campo: texto, opção, caminho ou botão.
- Na lista de consoles, `rename` e `remove` aparecem só em uma linha de script.
- Durante uma exportação, a linha mostra a tecla de parar antes do progresso.

**Filtro da árvore:** o campo ocupa a primeira linha do explorador, e a linha de status mostra `FILTER` durante a digitação.
Sem resultado, a árvore mostra `No match` e `Loaded catalogs only`.

**Avisos do banco:** um `NOTICE` do PostgreSQL ou um aviso do MySQL entra em **Messages** com o número da instrução. A aba mostra `Messages (n)`, e a aba ativa não muda.

**Idade da transação:** a linha de status mostra `Transaction open 3m` a partir de 1 min. Com 5 min ou mais, o texto usa a cor de erro.

**Sinal:** uma execução de `bellAfterSeconds` ou mais toca o BEL do terminal no fim. Um Stop não toca.

**Mensagens:** uma mensagem ocupa a linha de dicas por 8 s.
Uma tecla, uma colagem ou um clique remove a mensagem antes disso. O ponteiro e a roda não removem.
A espera de credenciais fica na linha enquanto a espera durar, e termina com ela.
Uma gravação que continua a falhar repete o alerta a cada nova tentativa.
A dica de configuração inválida fica até `Ctrl+W r` carregar um arquivo válido.

**Foco:** a etiqueta da barra de status nomeia o lugar das teclas: `EDITOR`, `RESULTS`, `EXPLORER`, `FILTER` ou `TABLE`.
No modo Vim, o editor mostra `NORMAL`, `INSERT` ou `VISUAL`.
O título do painel em foco usa a cor de destaque.
A linha atual do editor e da grade aparece somente no painel em foco.

**Barra de status:** o selo `PROD`, a transação e a execução nunca saem da barra.

**Confirmação da execução:** uma instrução `UPDATE` ou `DELETE` sem `WHERE` abre **Confirm the run**, com foco em **Cancel**.
Uma conexão de produção abre o mesmo diálogo para qualquer mudança. A janela mostra a contagem e a primeira instrução de risco.
Uma conexão somente leitura não pergunta, pois o banco recusa a escrita.

**Formatação:** `Ctrl+W f` formata a seleção ou a instrução sob o cursor, em um passo de desfazer.

**Comentário:** `Ctrl+W /` ou `Ctrl+/` comenta a seleção ou a linha do cursor, em um passo de desfazer. O cursor e a seleção continuam nos mesmos caracteres, então a mesma tecla desfaz a mudança.

Sem espaço, a barra remove nesta ordem: posição, dados, pool e schema. Depois, o nome da conexão encurta.
Uma transação aberta usa a cor de aviso. Uma transação com falha ou desconhecida usa a cor de erro.

**Grade:** o número da linha conta as linhas carregadas.
A borda de um cabeçalho aceita o arrasto nas três colunas entre duas colunas da grade.

**Barras de rolagem:** o editor e a grade reservam a última coluna.
O explorador usa a sua última coluna somente com excesso de linhas.

Os indicadores ficam na coluna livre, entre o número da linha e o texto:

| Símbolo | Estado |
|---|---|
| `●` | Executando |
| `✓` | Sucesso |
| `⋯` | Teto ou orçamento atingido |
| `✗` | Erro |
| `—` | Não executada |
| `■` | Parada |
| `◷` | Timeout |
| `?` | Resultado desconhecido |

Quando o banco informa a posição de um erro, o editor sublinha o token e mantém o cursor.
A marca sai com a próxima edição, e uma edição durante a execução impede a marca.

Esse exemplo mostra uma execução concluída, com duas instruções.
A segunda instrução atingiu o teto de linhas e encerrou a leitura.
A conexão já voltou ao pool, e o usuário percorre as linhas sem nova consulta.
Não mostrar sucesso definitivo de uma instrução antes de terminar sua leitura.

Regras dos controles:

- **Executar:** usa a seleção ou, sem seleção, a instrução sob o cursor.
- **Script inteiro:** executa o texto completo, parando no primeiro erro.
- **Parar script:** interrompe a sequência sem fechar o console, apagar o texto ou resolver a transação automaticamente.
- Não enviar as próximas instruções após aceitar a parada; não prometer interrupção imediata no servidor.
- Parar uma instrução ativa envia o cancelamento e aguarda 5 segundos pela resposta.
- Sem resposta, fechar a conexão; com transação aberta, pedir confirmação antes de fechar.
- Depois de fechar, indicar que o resultado da instrução é desconhecido.
- O seletor de banco ou schema vale para cada execução, mesmo com outra conexão física.
- Aceitar uma nova execução limpa as abas de resultados anteriores desse console, sem apagar o texto ou resolver a transação.
- A limpeza ocorre mesmo quando a nova execução precisa aguardar conexão; uma tentativa bloqueada não limpa os resultados.
- Cada resultado lê linhas até o teto, sem aguardar uma ação do usuário.
- Um resultado sem linhas mostra as colunas e a indicação de zero linhas.
- Ao atingir o teto, informar que o resultado pode ter mais linhas.
- Para ver mais linhas, aumentar o teto e executar o comando de novo.
- Bloquear outra execução, troca de modo e botões de commit ou rollback enquanto a execução estiver pendente ou ativa.
- Trocar o modo pelo seletor da barra ou pelo atalho.

O timeout SQL padrão é de 30 segundos, configurável, com opção de executar sem prazo.
Ele não substitui o prazo de conexão nem limita a espera no pool.
Contar o prazo por instrução, descontando a espera no pool e incluindo a leitura das linhas.

Depois da execução, uma transação aberta mantém sua conexão reservada.
Nesse estado, o usuário pode iniciar outra execução ou resolver a transação pelos controles disponíveis.

## 3. Visão de tabela

Duplo clique na tabela abre uma aba na mesma faixa dos consoles.
Abrir novamente a mesma tabela seleciona essa aba, sem duplicar.

```text
┌─ Conexões ────────┬─ console-1 ─ [users (table)] ──────────────────────────────────────┐
│ app (local)       │ app_db.public.users                                                │
│   public          │ WHERE    [active = true                                          ] │
│     users         │ ORDER BY [                                                       ] │
│     orders        │ [✓ Apply]  [✕ Clear]  [↓ Load more]  [■ Stop]                      │
│ app (global)      ├─────┬──────────┬───────────────────────────────────────────────────│
│ warehouse         │ id  │ name     │ active                                            │
│                   │ 101 │ Ana      │ true                                              │
│                   │ 102 │ Bruno    │ true                                              │
│                   │ ...                                                                │
│                   │ 200 rows loaded          [◀ Previous]  Page 2 of 2+  [▶ Next]      │
│                   │ No ORDER BY: rows can repeat or be missing between pages.          │
├───────────────────┴────────────────────────────────────────────────────────────────────┤
│ App (local) · Dados das grades: 1/256 MiB                                              │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

O exemplo mostra um filtro que o usuário já aplicou.
Manter filtros e ordenação somente enquanto essa aba permanecer aberta.
Fechar a aba descarta esses valores; reabrir começa sem filtros nem ordenação.
Uma aba de tabela da última sessão volta sem linhas, sem filtro e sem ordenação.
A primeira seleção da aba carrega as linhas. Uma aba que já começa na tela mostra `Enter loads the rows.` e espera `Enter` ou **Apply**, para a abertura não conectar nem pedir senha.
Uma tabela nova não recebe ordenação automática, nem por chave primária.

Regras da barra:

- Digitar não executa consultas.
- Recusar um filtro com outra instrução.
- `Ctrl+W x` para uma consulta da tabela em andamento.
- `Tab` aceita uma sugestão de autocomplete. A opção **Autocomplete** troca essa tecla por `Enter` ou pelas duas.
- `Enter` aplica o conteúdo dos campos, mesmo com autocomplete aberto.
- `[Aplicar]` tem o mesmo efeito de `Enter`.
- `[Limpar]` limpa os campos e aplica a consulta sem filtro nem ordenação.
- Clique no cabeçalho aplica a ordenação imediatamente, mantendo o `WHERE` já aplicado.
- Não aplicar um rascunho de `WHERE` como efeito do clique no cabeçalho.
- Filtro ou ordenação inválidos deixam a grade vazia, com o erro e os campos disponíveis para correção.

Implementação atual, de cima para baixo: título, campo `WHERE`, campo `ORDER BY`, botões
**Apply**, **Clear**, **Load more** e **Stop**, grade, resumo e uma linha de aviso.
O título aparece somente com 30 linhas ou mais. O uso do pool e dos dados fica na barra de status.
O rótulo da aba é `nome (table)` ou `nome (view)`.
**Definido:** a faixa mistura consoles e tabelas na ordem de abertura.
Fechar o último console com uma tabela aberta seleciona uma tabela, sem criar um console vazio.
O código ainda agrupa consoles antes das tabelas e cria um console vazio ao fechar o último.
O resumo mostra as linhas carregadas e o estado.
A tecla `c` conta as linhas do filtro aplicado, e o resumo passa a mostrar `100 of 12,345 rows loaded`. A tecla só age quando existem mais linhas para carregar.
A contagem usa o prazo, o Stop, a fila do pool e a senha de uma página, e não muda a grade. **Apply** e **Clear** limpam o total, e a ordenação mantém o total.
A linha de aviso mostra o risco da ordem instável, ou a mensagem da última consulta.
O erro de uma primeira página ocupa a área da grade.
O clique no cabeçalho ordena em ordem crescente, depois decrescente, e depois remove a ordem.

Rolagem e navegação percorrem os dados já carregados.
Mais linhas exigem **Carregar mais** ou **Next**.
Com mais de uma página, a linha sob a grade mostra **Previous**, `Page 2 of 3+` e **Next**. O `+` indica que a tabela pode ter mais linhas.
Cada página é um trecho das linhas acumuladas, e o botão coloca a primeira linha da página no topo da grade.
**Next** mostra uma página já carregada sem consulta. Se a página ainda não veio, **Next** carrega a página e depois a mostra.
**Previous** nunca consulta o banco. O número da página acompanha o cursor.
A linha prioriza o estado da carga. Sem espaço, os botões usam a forma curta `◀ 2/3+ ▶`, e a nota `more rows available` sai primeiro. Um estado longo tira os botões da linha, e `[` e `]` continuam a funcionar.
Carregar uma página faz uma nova consulta, sem manter uma leitura aberta entre páginas.
**Carregar mais** mantém o cursor e a rolagem atuais.
**Carregar mais** pode aguardar uma conexão livre.
Sem uma ordenação única, linhas podem repetir ou faltar entre as páginas.
Mostrar esse aviso quando não existir uma ordenação aplicada.
O teto global de 256 MiB pode suspender novas cargas, sem apagar as linhas existentes.
Se um valor ultrapassar o limite, indicar o motivo sem representá-lo como nulo, vazio ou conteúdo truncado sem aviso.
A cópia segue o formato TSV confirmado na seção 10 do plano, sem cabeçalhos.

### Aba de estrutura

`i` em uma linha do navegador, ou na grade de uma tabela, abre a aba de estrutura.
O rótulo da aba é `nome (structure)`. Abrir a mesma estrutura de novo seleciona a aba.
A aba da tabela e a aba de estrutura ficam abertas juntas.

```text
│ [console-1] [users (table)] [users (structure)]                                        │
│ app · users · structure                                                                │
│ [✓ Reload]  [↓ Load more]  [■ Stop]                                                    │
│ ─ Columns ─ Indexes ─ Foreign keys ──────────────────────────────────────────────────  │
│ name    type     nullable  default  key                                                │
│ id      integer  no                 PK                                                 │
│ email   text     no                                                                    │
│ active  boolean  yes       true                                                        │
```

| Linha do navegador | Conteúdo |
|---|---|
| Tabela, view ou coluna | Três seções: colunas, índices e chaves estrangeiras |
| Schema | Todas as colunas do schema, sem seções |
| Conexão | Todas as colunas dos schemas que o navegador mostra, sem seções |

| Grade | Colunas |
|---|---|
| Colunas da tabela | `name`, `type`, `nullable`, `default`, `key` |
| Índices | `name`, `columns`, `unique` |
| Chaves estrangeiras | `name`, `columns`, `references`, `on_update`, `on_delete` |
| Schema ou conexão | `schema`, `relation`, `name`, `type`, `nullable`, `default` |

- O título de uma conexão mostra `all schemas` no lugar do nome.
- A aba não tem os campos `WHERE` e `ORDER BY`, e um clique no cabeçalho não ordena.
- `[` e `]`, ou um clique no nome da seção, trocam a seção. A troca relê a grade.
- Em um schema ou uma conexão, `[` e `]` mudam a página.
- **Reload** relê a grade. DDL não atualiza a aba sozinho.
- A grade usa a paginação, o orçamento, a busca, a cópia e a exportação da aba de tabela.
- A exportação não tem tabela de destino, como em um console.
- A aba não volta na próxima sessão.
- Redis e Valkey não têm estrutura. `i` mostra o motivo na barra de status.

### Exportação

`e` na grade, ou **Export the result to a file** na paleta, abre o modal. A exportação cobre o resultado ativo e gera um arquivo.

```text
╭─ Export ───────┬─────────────────────────────────────────────────────╮
│ > CSV          │   Rows             < All rows >                     │
│   TSV          │   File             sers-2026-10-31-1530.csv [...]  │
│   JSON         │   Header           < On >                           │
│   JSON Lines   │   Delimiter        < , >                            │
│   SQL INSERT   │   NULL text        < Empty >                        │
│                │   UTF-8 BOM        < Off >                          │
│                │                                                     │
│                │   [Export]  [Cancel]                                │
╰────────────────┴─────────────────────────────────────────────────────╯
```

| Alcance | O que grava |
|---|---|
| **Selection** | As linhas e as colunas selecionadas |
| **Loaded rows** | As linhas da grade |
| **All rows** | Todas as linhas e colunas, com uma nova leitura. A tabela usa o filtro e a ordem aplicados |

O modal abre em **Selection**, se existir. Senão, abre em **All rows**, se disponível, e por último em **Loaded rows**.
**All rows** some quando a grade já tem tudo.
No console, **All rows** exige uma leitura pura e um comando na execução, ou uma transação aberta. Nos outros casos, o modal explica o motivo.
Redis e Valkey oferecem só as linhas carregadas, sem SQL INSERT.

| Formato | Opções |
|---|---|
| CSV | Cabeçalho, delimitador, texto do NULL e BOM |
| TSV | Cabeçalho, texto do NULL e BOM |
| JSON e JSON Lines | Nenhuma |
| SQL INSERT | Tabela, um por linha ou agrupado, e linhas por `INSERT` |

O modal tem 72 colunas e cresce até 140 numa tela maior, com 8 colunas livres. A altura não muda, pois ela já mostra todos os campos.
O campo **File** aceita `~` e um caminho relativo à pasta atual, e cria as pastas que faltam. A extensão segue o formato até o usuário editar o caminho.
Um arquivo existente pede **Replace** ou **Cancel**.
O botão `...` e `Ctrl+O` abrem o explorador de arquivos.

Depois de **Export**, o modal fecha, a aba fica ocupada, e a linha de dicas mostra as linhas gravadas. Cada aba pode ter uma exportação, e o modal recusa um destino em uso.
**All rows** não tem limite de tempo, e **Stop** cancela. A grade mantém o resultado.
A gravação usa `<nome>.tmp` e renomeia no fim. Depois de **Stop** ou de um erro, uma janela oferece **Delete** e **Keep**. Sem linhas gravadas, o temporário some sem pergunta.

### Explorador de arquivos

Os campos de caminho têm um botão `...`. O botão e `Ctrl+O` abrem esta janela acima do modal:

```text
╭ Choose a file ───────────────────────────────────────────────╮
│ /home/ana/projetos/loja                                      │
│ >                                                            │
│ Use this folder                                              │
│ ../                                                          │
│ docs/                                                        │
│ src/                                                         │
│ Cargo.toml                                                   │
│ loja.db                                                      │
╰──────────────────────────────────────────────────────────────╯
```

A janela fica 8 colunas mais estreita que o modal, e a lista mostra de 14 a 35 linhas, conforme a altura da tela.
Os campos são **File** do Export, **File** do SQLite, **CA file**, **Client certificate** e **Client key**.
A janela abre na pasta do caminho do campo. Sem ela, abre na pasta existente mais próxima, e com o campo vazio abre na pasta atual.
As pastas vêm primeiro, e a ordem ignora maiúsculas. Todos os arquivos aparecem, menos os ocultos. Um filtro que começa com `.` mostra os ocultos.
`Enter` numa pasta entra nela. `Enter` num arquivo preenche o campo com o caminho absoluto e fecha a janela. `Esc` fecha a janela, e o campo não muda.

**Use this folder** aparece só no Export e no SQLite, pois esses campos aceitam um arquivo novo. A linha troca a pasta e mantém o nome do campo. Com o campo vazio, ele recebe a pasta, e o usuário digita o nome.
No Export, a extensão continua a seguir o formato depois de **Use this folder**, e para de seguir depois de escolher um arquivo.
Fechar a aba ou sair apaga o temporário.

### Chave Redis ou Valkey

No navegador, cada banco numerado mostra até 1.000 chaves. `(partial list)` ao lado do banco indica uma lista incompleta.
`Enter` em uma chave abre o valor em uma aba com o rótulo `<chave> (key)`.
A aba usa a grade, as páginas e **Load more**, sem `WHERE`, `ORDER BY`, **Clear** e ordenação.
**Reload** lê o valor de novo.
No console, cada linha é um comando, como no `redis-cli`. Um `#` no início da linha faz um comentário.

## 4. Cadastro de conexão

Oferecer formulário e entrada por URI, com a mesma política de credenciais.
Com projeto ativo, selecionar projeto por padrão e permitir escolher global.
Sem projeto ativo, oferecer somente global.
O exemplo abaixo considera um projeto ativo.

```text
┌─ Conexão ──────────────────────────────────────────────┐
│ Tipo:       [PostgreSQL]                               │
│ Nome:       [app]                                      │
│ Escopo:     [Projeto]  (pode alterar para Global)       │
│ Entrada:    [Campos / URI]                             │
│                                                       │
│ Host:       [db.example.test]                          │
│ Porta:      [5432]                                     │
│ Banco:      [app_db]                                   │
│ Usuário:    [app_user]                                 │
│ Credencial: [Somente nesta sessão]                    │
│ TLS:        [Verificar certificado]                    │
│ CA:         [Do sistema]                               │
│ Certificado de cliente: [Nenhum]                       │
│ Pool:       [5]                                        │
│ Somente leitura: [ ]                                   │
│ Produção:   [ ]                                        │
│ Conectar:   prazo [30] segundos                        │
│ Tema:       [Padrão]                                   │
│ Ícone:      [Padrão]                                   │
│                                                       │
│ [Testar conexão]  [Salvar]  [Cancelar]                │
└───────────────────────────────────────────────────────┘
```

Os campos mudam conforme o tipo. SQLite usa arquivo, não banco em memória.

Implementação atual: o formulário agrupa os campos em **General**, **Address**, **Credentials**, **Security** e **Options**.
O formulário escolhe o primeiro arranjo que cabe na tela, com 2 linhas e 2 colunas de folga:

| Arranjo | Altura com PostgreSQL | Largura |
|---|---|---|
| Uma coluna, com títulos e espaço entre os grupos | 33 | 72 |
| Duas colunas, com títulos e espaço entre os grupos | 22 | 116 |
| Uma coluna, com espaço entre os grupos | 28 | 72 |
| Uma coluna, sem espaço | 22 | 72 |

Numa tela maior, a largura cresce até 140 colunas e deixa 8 colunas livres. O arranjo não muda, pois a largura cresce depois da escolha.

O campo em foco tem uma marca no início da linha, rótulo em destaque e fundo próprio.
Sem cor, a marca é `>`, e o rótulo fica em negrito.
A nota do formulário termina na linha acima dos botões. Uma nota longa ocupa as duas linhas da área.
O sucesso do teste usa a cor de sucesso. Uma nota de andamento usa a cor de aviso, e um erro usa a cor de erro.
Resolver caminhos relativos do `config.json` conforme o escopo: raiz do projeto ou pasta global de configuração.
No formulário, um arquivo SQLite digitado parte do diretório atual, e a conexão grava o caminho completo.
Um caminho salvo e não alterado continua como está no arquivo, e o teste usa a pasta do escopo.

O formulário sugere TLS desativado em `localhost`, `127.0.0.0/8`, `::1` e socket Unix, e verificação nos demais.
O campo **Somente leitura** é aplicado pelo banco e evita acidentes.

O campo **Production** mostra `PROD` na barra de status, inclusive com `NO_COLOR`.
Ele também pede confirmação antes de uma execução que não seja leitura simples.
O campo muda com a conexão ativa, como o nome e o tema.

Exigir nome obrigatório e único por escopo, sem distinguir maiúsculas e minúsculas, inclusive ao renomear.
Permitir nomes iguais entre global e projeto, com indicação de origem quando houver colisão.

Exigir desconexão antes de alterar dados de acesso, TLS ou limite do pool de uma definição conectada.
Uma URI não autoriza salvar senha na configuração do projeto.
Não mostrar senhas em exemplos, mensagens de erro ou representações públicas da conexão.

## 5. Confirmações e erros

### Transação aberta

Ao fechar um console com transação aberta, oferecer commit, rollback ou cancelamento do fechamento.
Na saída da aplicação, perguntar separadamente para cada console.
Executar a escolha antes de apresentar a pergunta do próximo console.
Cancelar depois não desfaz operações já concluídas.

Ao ativar autocommit com transação aberta, usar as mesmas três decisões para concluir ou cancelar a troca.
Não selecionar commit como consequência automática da solicitação de troca.

Se o commit ou rollback do fechamento falhar, manter o console aberto e mostrar o erro.
Oferecer nova tentativa, quando a sessão original e o estado da transação permitirem, ou cancelar o fechamento.
Não repetir a operação automaticamente nem usar outra conexão para tentar concluir a transação anterior.
Se perder a conexão sem confirmar o resultado, indicar que o resultado é desconhecido.
Durante a saída, resolver a falha atual antes de perguntar sobre outro console.

### Conexão

- Conectar no primeiro uso, sem abrir conexões ao iniciar a aplicação.
- Oferecer a ação **Desconectar** em cada definição conectada.
- Antes da primeira conexão a um destino nesta máquina, mostrar host, porta, usuário e TLS.
- Perguntar de novo quando um desses valores mudar.
- Após perder uma transação aberta, marcar o estado como desconhecido e exigir confirmação antes da próxima execução.

### Leitura e fechamento

- Fechar um console ativo pede confirmação para parar a sequência após finalizar a instrução atual.
- Não permitir fechar um resultado enquanto sua instrução ainda lê linhas; usar **Parar script**.
- Não prometer cancelamento imediato do comando em andamento no servidor.

### Arquivo e configuração

- Excluir um console salvo ou uma conexão exige confirmação dos itens afetados.
- Fechar uma aba não exclui seu script salvo.
- Remover uma conexão preserva seus scripts em disco, sem exclusão em cascata ou reassociação automática por nome.
- Mostrar esses scripts na lista Scripts sem conexão, permitindo abrir e editar sem executar SQL.
- Exigir escolha e confirmação de uma conexão antes de executar; associar não executa SQL automaticamente.
- Se a associação mudar o escopo, informar a nova localização do arquivo antes de concluir.
- Falha de salvamento mantém o texto em memória e mostra um alerta.
- Fechar com alterações não gravadas exige confirmação explícita.
- Alteração externa do script exige escolha entre recarregar e sobrescrever.
- Uma configuração JSON inválida mostra arquivo e posição do erro, sem liberar operações com bancos até a correção.
- O usuário corrige esse arquivo fora da aplicação e usa **Recarregar**.
- `workspace.json` ilegível usa o estado inicial e mostra um aviso não bloqueante.
- A ação para um script removido usa o rótulo **Fechar e descartar texto**.

### Credenciais

Usar o cofre como armazenamento principal.
Somente a ausência de cofre permite oferecer gravação global em texto simples, mediante autorização explícita.
Cofre bloqueado ou com erro não ativa essa exceção.
Com o cofre bloqueado ou com erro, oferecer **Tentar novamente**, **Usar senha nesta sessão** e **Cancelar**.
Tratar uma falha desconhecida como erro, nunca como ausência.
Confirmar a distinção em testes reais antes de liberar texto simples como alternativa.
Senha de sessão permanece em memória, por conexão, até fechar a aplicação.
Se a gravação falhar, manter a senha em memória, informar que ela não foi salva e oferecer nova tentativa explícita.
Quando um cofre passar a existir, pedir confirmação para migrar e verificar a gravação antes de oferecer remover a senha global.
Falha na migração não autoriza apagar a senha anterior.

Usar identidades estáveis ao mover um projeto.
Pedir confirmação antes de reutilizar credenciais em cópias ou quando não for possível distinguir cópia de mudança de pasta.
Em outro computador sem a credencial, solicitar a senha, sem transportar segredos pelo projeto.
Mudar o destino não autoriza enviar a senha anterior automaticamente.
Remover uma conexão não apaga sua senha do cofre; excluir essa senha exige confirmação separada.
Avisar que outras cópias do projeto podem usar a mesma credencial.

Solicitar a senha antes do primeiro uso que precisar dela, inclusive teste e leitura do catálogo.
Iniciar com uso somente na sessão. Salvar exige uma escolha explícita.
A solicitação de senha, a confiança e a recuperação de falhas do cofre já existem no código.
A senha fica mascarada, e a opção de salvar começa desmarcada.
O formulário permite sessão ou cofre; testar uma senha digitada não a grava nem substitui a senha da conexão salva.
Um caminho novo de projeto exige confirmação também ao testar ou salvar uma definição existente.

#### Fluxos de credenciais

Estes fluxos seguem regras aprovadas.

| Fluxo | Conteúdo necessário | Regra de segurança |
|---|---|---|
| Texto simples global | Aviso antes da autorização, opção desmarcada e identificação de `credentials.json` na pasta global | Oferecer somente com ausência confirmada de cofre; nunca gravar o segredo no projeto |
| Migração | Confirmação para migrar e, após conferência, oferta de remoção da senha global | Preservar a fonte em caso de falha e informar migração incompleta |
| Senha recusada | Solicitação de outra senha uma vez e indicação de suspensão após a segunda recusa; durante a suspensão, cada ação explícita abre a solicitação | Não confundir recusa com falha de rede, TLS ou execução SQL; não tentar conectar sozinha durante a suspensão |
| Exclusão da credencial | Confirmação separada, com identificação da conexão e aviso sobre outras cópias do projeto | Remover a conexão não autoriza apagar o segredo |

Os testes de tela atuais não validam os diálogos nativos do Keychain nem do Secret Service.
A [matriz do plano](../PLAN.md#fechamento-da-etapa-de-credenciais) separa implementação e validação pendentes.

## 6. Navegação e foco

O modo convencional é padrão. O modo Vim também atua na navegação da interface.
O mouse funciona nos dois modos.

Contextos que precisam de tratamento separado:

| Contexto | Comportamento |
|---|---|
| Navegador | Selecionar conexão ou objeto; abrir tabela por duplo clique |
| Abas principais | Alternar entre consoles e tabelas já abertos |
| Editor | Editar SQL; `Tab` aceita sugestões e `Enter` quebra a linha, salvo outra escolha em **Autocomplete**; executar somente por uma ação explícita |
| Filtros | `Tab` completa; `Enter` aplica |
| Resultados | Selecionar e copiar dados; navegar somente nas linhas carregadas |
| Diálogo | Resolver a pergunta sem acionar comandos de execução ao fundo |

O autocomplete inicial oferece schemas, tabelas, views e colunas; funções e assinaturas ficam fora desse escopo.
Sem catálogo disponível, sugerir as palavras-chave do dialeto.
Os atalhos de foco, abas e execução estão definidos em [KEYMAP.md](KEYMAP.md).
Atalhos de navegação não capturam caracteres durante a inserção de texto.
Dados, nomes e erros externos não devem ser interpretados como sequências de controle do terminal.

## 7. Retorno ao console

Reabrir um console recupera seu texto, cursor e rolagem.
Não recuperar dados das grades nem executar SQL automaticamente.
Se o console já estiver aberto, apenas selecionar a aba existente.
Se estiver em uso por outra instância, bloquear sua abertura nesta instância.

## 8. Esboços complementares

### Transação manual aberta

A aba **Mensagens** mostra avisos, erros e comandos sem linhas.

```text
┌─ console-1 ──────────────────────────────────────────────┐
│ Schema: [public]                                         │
│ [Executar] [Script inteiro] [Parar script]               │
│ Modo: [Manual] · Transação aberta · [Commit] [Rollback]  │
├──────────────────────────────────────────────────────────┤
│ 1 ✓ UPDATE users SET active = false WHERE id = 7;        │
├──────────────────────────────────────────────────────────┤
│ Mensagens                                                │
│ 1 linha alterada. Transação ainda não confirmada.        │
└──────────────────────────────────────────────────────────┘
```

### Espera no pool

**Parar script** cancela o aguardo sem enviar SQL ao banco.

```text
┌─ console-2 ──────────────────────────────────────────────┐
│ Aguardando conexão · Pool ocupado: 5/5                   │
│ O SQL ainda não foi enviado ao banco.                    │
│ [Parar script]                                           │
└──────────────────────────────────────────────────────────┘
```

### Cancelamento

O primeiro esboço mostra a espera de 5 segundos pela resposta da instrução.
O segundo aparece somente com transação aberta.

```text
┌─ Parar script ───────────────────────────────────────────┐
│ Cancelamento enviado. Aguardando a resposta do banco.    │
│ Tempo: 3 de 5 segundos                                   │
└──────────────────────────────────────────────────────────┘
```

```text
┌─ Fechar conexão ─────────────────────────────────────────┐
│ O banco não respondeu ao cancelamento.                   │
│ Fechar a conexão encerra a transação aberta.             │
│ As alterações não confirmadas serão perdidas.            │
│                                                          │
│ [Fechar conexão]  [Continuar aguardando]                 │
└──────────────────────────────────────────────────────────┘
```

### Orçamento esgotado

Nos consoles, o orçamento encerra a leitura como o teto de linhas.
Na visão de tabela, **Carregar mais** fica indisponível até existir espaço.

```text
┌─ Dados das grades ───────────────────────────────────────┐
│ Orçamento atingido: 256/256 MiB                          │
│ As linhas carregadas foram mantidas.                     │
│ Feche abas ou aumente o limite para carregar mais linhas. │
└──────────────────────────────────────────────────────────┘
```

### Cadastro de SQLite

Se o arquivo não existir, perguntar antes de criá-lo.

```text
┌─ Conexão ────────────────────────────────────────────────┐
│ Tipo:         [SQLite]                                   │
│ Nome:         [local]                                    │
│ Escopo:       [Projeto]  (pode alterar para Global)      │
│ Arquivo:      [data/app.db]                              │
│ foreign_keys: [Padrão do SQLite]                         │
│ Bloqueio:     prazo [5] segundos                         │
│ Pool:         [5]                                        │
│                                                          │
│ [Testar conexão]  [Salvar]  [Cancelar]                   │
└──────────────────────────────────────────────────────────┘
```

### Entrada por URI

Uma senha na URI segue as regras de credenciais e não fica na configuração do projeto.

```text
┌─ Conexão ────────────────────────────────────────────────┐
│ Tipo:    [PostgreSQL]                                    │
│ Nome:    [app]                                           │
│ Escopo:  [Projeto]  (pode alterar para Global)           │
│ Entrada: [URI]                                           │
│ URI:     [postgres://app_user@db.example.test/app_db]    │
│                                                          │
│ [Testar conexão]  [Salvar]  [Cancelar]                   │
└──────────────────────────────────────────────────────────┘
```

### Edição de conexão conectada

Dados de acesso, TLS e limite do pool exigem desconexão.

```text
┌─ Editar conexão: app (local) ────────────────────────────┐
│ Esta conexão está conectada.                             │
│ Desconecte para alterar acesso, TLS ou limite do pool.   │
│                                                          │
│ Nome: [app]                                              │
│                                                          │
│ [Desconectar]  [Salvar nome]  [Cancelar]                 │
└──────────────────────────────────────────────────────────┘
```

### Confirmação de destino

Esta pergunta aparece antes da primeira conexão a um destino nesta máquina, conforme a seção 3 do plano.
Mostrar destino, usuário e TLS; iniciar com foco em **Cancelar**.
A confirmação já existe no código. Um caminho novo pode mostrar **Confirm project path**.
Cancelar impede o envio da operação que espera essas credenciais.

```text
┌─ Confirmar destino ──────────────────────────────────────┐
│ Host:    db.example.test                                 │
│ Porta:   5432                                            │
│ Usuário: app_user                                        │
│ TLS:     Verificar certificado                           │
│                                                          │
│ [Confirmar]  [Cancelar]                                  │
└──────────────────────────────────────────────────────────┘
```

### Solicitação de senha

Iniciar com uso somente na sessão, com a opção de salvar desmarcada.
O texto da opção muda conforme o cofre.
Sem cofre, a opção informa que a gravação global usa **texto simples** e exige autorização.
Cofre bloqueado ou com erro não permite essa alternativa.

```text
┌─ Senha: app (local) ─────────────────────────────────────┐
│ Usuário: app_user                                        │
│ Senha:   [••••••••]                                      │
│                                                          │
│ [ ] Salvar no cofre do sistema                           │
│                                                          │
│ [Conectar]  [Cancelar]                                   │
└──────────────────────────────────────────────────────────┘
```

### Scripts sem conexão

A execução fica bloqueada até a escolha e a confirmação de uma conexão.

```text
┌─ Scripts sem conexão ────────────────────────────────────┐
│ investigar-pedidos       ontem, 16:20                    │
│ relatorio-mensal         12/10, 09:15                    │
│                                                          │
│ [Abrir]  [Associar a uma conexão]                        │
└──────────────────────────────────────────────────────────┘
```

### Configurações

Abrir com `Ctrl+W ,`.
A janela tem 72 colunas e cresce até 140 numa tela maior, com 8 colunas livres. A altura não muda.
As listas, os conflitos, **Rename console**, a senha e as perguntas crescem até 100 colunas. As listas e os conflitos também crescem até 30 linhas. O detalhe da célula e a ajuda crescem até 140 colunas.
O projeto grava somente os valores que declara.
Oferecer escopo global ou do projeto e os sete campos abaixo.
Mostrar os valores herdados do global e permitir remover uma definição do projeto para usar o global.
**Salvar** grava as alterações de todas as seções. **Cancelar** não altera nada.
A barra lateral agrupa os campos nas seções **General**, **Editor** e **Results**. O painel mostra uma seção por vez.
O escopo e os botões ficam fora das seções, e aparecem em todas elas.
Uma nota de erro aparece acima dos botões, mesmo quando o campo está em outra seção.
Uma seção nova precisa de uma linha na tabela `SECTIONS`, e a janela cresce com a maior seção.
Manter `config.json` para opções avançadas, como o `keymap`.
Ao mudar o modo de teclado, preservar texto, cursor e rolagem.
A troca acontece no próprio editor, então a seleção e o histórico continuam, inclusive na recarga.

```text
╭─ Settings ─────┬─────────────────────────────────────────────────────╮
│                │                                                     │
│ ▌ General      │   Scope         < Project >                         │
│   Editor       │                                                     │
│   Results      │   GENERAL                                           │
│   Appearance   │   Keyboard      < from global: Vim >                │
│                │   Reopen tabs   < Default: On >                     │
│                │                                                     │
│                │                                                     │
│                │                                                     │
│                │                                                     │
│                │   [Save]  [Cancel]                                  │
│                │                                                     │
╰────────────────┴─────────────────────────────────────────────────────╯
```

A seção **Appearance** tem o campo **Default theme**.
A escolha muda as cores na hora, antes de salvar. `Esc` e **Cancel** trazem de volta o tema salvo.
No escopo global, um tema definido no projeto continua com prioridade.

O formulário de conexão tem o campo **Theme**, que define um tema só para aquela conexão.
O valor **Default** usa o tema padrão. A aba aberta decide as cores da tela inteira: a tela usa o tema da conexão da aba, e uma aba sem tema próprio usa o tema padrão.
Esse campo também muda as cores antes de salvar. Ele funciona com a conexão ativa e não fecha a sessão nem a transação.
Enquanto Settings está aberto, a tela mostra o tema padrão escolhido, mesmo em uma aba com tema próprio.

O campo **Icon** troca o ícone da conexão no explorador. O valor **Default** usa o ícone do banco.
Os ícones são `⚠ ☠ ☢ ⚑ ♛ ⚗ ★ ◆ ⚒ ⚙ ⌂ ♥ ❄ ✔`, e o nome no arquivo é o nome em inglês em minúsculas, como `skull`.
O ícone escolhido fica antes do nome, e a marca do banco (`pg`, ou o logo com a Nerd Font) fica depois dele:

```text
 ▸ ● ☠ prod  pg
```

Esse campo também funciona com a conexão ativa e aparece em todos os conjuntos de ícones, inclusive com `NO_COLOR`.

| Tema | Valor em `theme` | Uso |
|---|---|---|
| Auto | `auto` | Astrodark com truecolor; cores do terminal nos demais casos |
| Astrodark | `astrodark` | Escuro neutro |
| Terminal colors | `ansi` | Cores do perfil do terminal, sem truecolor |
| Tokyo Night | `tokyo-night` | Escuro azulado |
| Catppuccin Mocha | `catppuccin-mocha` | Escuro violeta |
| Catppuccin Latte | `catppuccin-latte` | Claro |
| Gruvbox | `gruvbox` | Escuro quente |
| Nord | `nord` | Cinza azulado |

`NO_COLOR` tem prioridade sobre qualquer tema.

### Fechamento com transação aberta

O foco inicial fica em **Cancelar fechamento**, a opção sem efeito no banco.
Não existe commit como resposta padrão.

```text
┌─ Fechar console-1 ───────────────────────────────────────┐
│ Este console tem uma transação aberta.                   │
│                                                          │
│ [Commit e fechar]  [Rollback e fechar]                   │
│ [Cancelar fechamento]                                    │
└──────────────────────────────────────────────────────────┘
```

### Janela de detalhe

`Enter` na grade abre o valor completo. A janela é somente leitura.

```text
┌─ users.bio · linha 2 ────────────────────────────────────┐
│ Texto completo do valor selecionado, com quebra de       │
│ linha automática e rolagem.                              │
│                                                          │
│ 1.240 caracteres · texto                                 │
│ [Copiar]  [Fechar]                                       │
└──────────────────────────────────────────────────────────┘
```

Um objeto ou array JSON válido de até 1 MiB abre formatado, e `f` troca entre o texto formatado e o original.
A formatação só muda espaços: as chaves mantêm a ordem, e os números mantêm os dígitos.
`Ctrl+C` copia a forma visível, e o `Ctrl+C` da grade copia o valor original.
A opção **Format JSON** do diálogo Settings escolhe a forma inicial. O padrão é ligado.

### Janela de linha

`r` na grade abre a linha do cursor na vertical. A janela é somente leitura.

```text
╭─ row 3 of 100 loaded ───────────╮
│ id         : 3                  │
│ name       : Ana Souza          │
│ bio        : texto longo que    │
│              quebra alinhado    │
│ deleted_at : NULL               │
╰─────────────────── 12 columns ──╯
```

`Left` e `Right` mudam de linha, sem ler outra página, e o cursor da grade acompanha a linha.
A janela mantém a rolagem, então as mesmas colunas ficam à vista ao comparar duas linhas.
Cada valor mostra até 4.096 colunas e termina com `…` no corte. `Enter` na grade mostra o valor inteiro.
O JSON fica compacto nesta janela.

### Tela de ajuda

A tela mostra os atalhos do modo ativo e do painel em foco.

```text
┌─ Ajuda · Convencional · Editor ──────────────────────────┐
│ Ctrl+Enter ou F5   Executar                              │
│ Shift+Enter        Script inteiro                        │
│ Ctrl+W x           Parar script                          │
│ Ctrl+Space         Sugestões                             │
│ Ctrl+F             Buscar e substituir                   │
│ Ctrl+W h/j/k/l     Mudar de painel                       │
│                                                          │
│ Esc  Fechar                                              │
└──────────────────────────────────────────────────────────┘
```

### Menu do prefixo

`Ctrl+W` abre esta janela acima da barra de status. A janela mostra as teclas efetivas do `keymap`.

```text
╭─ Ctrl+W ─────────────────────────────────────────────────────────╮
│ Panels                Run                    Connection          │
│ hjkl Move the focus   e    Run the statement d    Schema or da…  │
│ b    Toggle the expl… s    Run the script    a    Change conn…   │
╰──────────────────────────────────────────────────────────────────╯
```

### Histórico de consultas

`Ctrl+W H` abre a janela **Query history**, com as instruções da conexão do console, a mais nova primeira.
Cada linha mostra sucesso ou falha, a idade e o início do SQL. As palavras digitadas filtram a lista.
`Enter` insere a instrução no cursor e não executa. A opção **Query history** do diálogo Settings desliga o histórico.

### Paleta de comandos

`Ctrl+P` abre esta janela. Cada palavra do filtro precisa aparecer no nome ou na tecla.
A janela tem 64 colunas e 12 linhas de lista, e cresce até 100 colunas e 30 linhas numa tela maior. Ela continua no topo.

```text
╭─ Commands ───────────────────────────────────────────╮
│ > conn                                               │
│ Change the connection of the console        Ctrl+W a │
│ Add a connection                                     │
│ Scripts without a connection                         │
╰──────────────────────────────────────────────────────╯
```

### Tela inicial

A tela aparece sobre um editor sem texto. Ela não guarda estado e não captura teclas.
O primeiro caractere digitado remove a tela.

```text
           SQLN 0.1.0
           Write SQL here

                   F5  Run the statement
             Shift+F5  Run the script
           Ctrl+Space  Show suggestions
               Ctrl+P  Show all commands
                   F1  Show all keys
```

Sem conexão, a tela mostra **No connection yet** e o caminho `Ctrl+W h a`.

### Terminal pequeno

A aplicação preserva o estado e volta a desenhar quando o tamanho for suficiente.

```text
┌─ SQLN ───────────────────────────────────────────────────┐
│ Terminal pequeno: 52 x 14                                │
│ Tamanho mínimo: 80 x 24                                  │
│ Execuções e transações continuam ativas.                 │
└──────────────────────────────────────────────────────────┘
```

### Alterações externas em lote

Cada linha exige uma escolha. O salvamento dos arquivos listados fica pausado.

```text
┌─ Scripts alterados fora do SQLN ─────────────────────────┐
│ investigar-pedidos       [Recarregar] [Sobrescrever]     │
│ relatorio-mensal         [Recarregar] [Sobrescrever]     │
│ console-1 (removido)     [Recriar]    [Fechar]           │
│                                                          │
│ [Aplicar]  [Decidir depois]                              │
└──────────────────────────────────────────────────────────┘
```

### Busca e substituição no editor

`Ctrl+F` abre a barra. `Tab` passa para o campo de substituição.
As teclas da barra estão em [KEYMAP.md](KEYMAP.md).

```text
┌─ console-1 ──────────────────────────────────────────────┐
│ Buscar:     [users            ]  3 de 12                 │
│ Substituir: [people           ]  [Substituir] [Todos]    │
├──────────────────────────────────────────────────────────┤
│ 2 SELECT id FROM users;                                  │
│ 3 SELECT name FROM users;                                │
└──────────────────────────────────────────────────────────┘
```
