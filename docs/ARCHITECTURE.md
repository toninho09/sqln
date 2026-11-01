# SQLN — Arquitetura

## Estado deste documento

**Definido:** este documento está aprovado.
Ele detalha a arquitetura do [plano](../PLAN.md).
Os nomes e as assinaturas são exemplos, não contratos finais.
Os fatos sobre os drivers vêm de [spikes/RESULTS.md](../spikes/RESULTS.md).

## 1. Visão geral

O projeto é um único crate Rust.
A interface roda na thread principal e nunca espera o banco.
Cada console tem uma tarefa própria, dona da sessão reservada. Este documento chama essa tarefa de ator.
O gerente de execução guarda um pool por conexão, e todas as conexões usam um único runtime e um único canal de eventos.

```text
Thread principal                    Runtime Tokio
┌─────────────────────┐  Comando   ┌───────────────────────────┐
│ Eventos do terminal │ ─────────▶ │ Gerente de execução       │
│ Roteador de teclas  │            │  ├── Script runner        │
│ Estado da aplicação │ ◀───────── │  └── Pool por definição   │
│ Desenho             │  Evento    │       └── Ator de conexão │
└─────────────────────┘            └───────────────────────────┘
                                     Thread própria: rusqlite
```

## 2. Módulos

| Módulo | Responsabilidade |
|---|---|
| `app` | Estado da aplicação, ações e atualização do estado |
| `ui` | Desenho das telas e dos widgets, temas (`theme`), esquemas de cores (`schemes`), símbolos (`icons`), barra de status (`statusline`) e menu do prefixo (`which_key`) |
| `palette` | Estado e filtro da paleta de comandos; `app` monta a lista, e `ui` desenha |
| `input` | Mapa de teclas: regiões, ações, sequências e regras de `keymap` |
| `navigator` | Árvore de conexões, schemas, relações e colunas |
| `form` | Campos e validação do cadastro de conexão |
| `connection` | Definições de conexão e o estado de cada conexão na aplicação |
| `editor` | EdTUI, camada convencional, busca e substituição |
| `sql` | Tokenizador, separação e classes de instruções |
| `complete` | Sugestões do autocomplete |
| `catalog` | Cache em memória; schemas e tabelas primeiro, colunas sob demanda |
| `console` | Editor, resultados, mensagens e estado de um console |
| `result` | Grade de um resultado com cursor e busca, usada por consoles e tabelas |
| `table` | Aba de tabela: páginas, campos, ordenação, sugestões e consulta |
| `line_input` | Campo de texto de uma linha, usado pelos campos da barra de filtro |
| `exec` | Execuções, scripts, teto e cancelamento |
| `pool` | Limite, fila, reserva e limpeza |
| `connector` | Contrato e conectores PostgreSQL, MySQL, SQLite e Redis/Valkey |
| `grid` | Linhas, larguras, busca, orçamento e TSV |
| `export` | Formatos de exportação, arquivo temporário e formulário do modal |
| `find` | Comparação de texto das duas buscas |
| `clipboard` | Área de transferência do sistema e OSC 52 |
| `terminal` | Preparação e restauração do terminal |
| `store` | Pastas dos escopos, `config.json`, scripts dos consoles, `workspace.json`, gravação atômica e bloqueios |
| `script_file` | Estado salvo de um console: hash, fim de linha, espera do salvamento e problema de gravação |
| `creds` | Cofre e senhas de sessão, com `zeroize` |
| `trust` | Destinos confirmados e caminhos conhecidos |
| `settings` | Formulário das preferências por escopo |
| `log` | Registro opcional de eventos, sem SQL nem valores |

`ui` e `input` não dependem de `connector`.
Somente `connector` conhece tipos dos drivers.

`app::auth` integra `creds` e `trust` ao primeiro uso, com os diálogos de senha e confiança.
`creds` oferece identidades, senhas de sessão e os adaptadores do cofre.
`trust` oferece a leitura e a gravação de confirmações globais.
As chamadas do cofre são síncronas, mas `Manager::spawn_blocking` as executa fora da thread da interface.
`app` aplica a prioridade das teclas e o mouse. `app/auth.rs`, `app/keys.rs`, `app/close.rs`, `app/connections.rs`, `app/mouse.rs`, `app/tables.rs`, `app/persist.rs`, `app/scripts.rs`, `app/session.rs` e `app/conflicts.rs` dividem esse código.
`store` guarda as pastas e os arquivos. Ele não conhece `app`, `ui` nem `connector`.

### Arquivos e salvamento

`store::Store` tem a pasta global e, quando existe, a pasta do projeto. Cada pasta é um `Root`, com a pasta do `config.json` (`dir`) e a base dos caminhos relativos (`base`). A base do projeto é a raiz, e não `.sqln/`.

| Parte | Responsabilidade |
|---|---|
| `store::file` | `write_atomic`, `write_private_atomic` (temporário exclusivo e permissão `0600` no Unix), hash, bloqueios e limpeza dos `.tmp` antigos |
| `store::config` | Leitura com `serde`, validação, precedência, `Definition` de cada entrada e `change_connections` (releitura sob bloqueio, mudança e gravação) |
| `store::scripts` | Arquivos `.sql` e `.json` de um console, lista, bloqueio por console, cópia entre escopos e fim de linha |
| `store::workspace` | `workspace.json`, lido sem erro e gravado sob o bloqueio de `config.lock` |
| `script_file::ScriptFile` | Hash do último conteúdo gravado, espera de 500 ms, nova tentativa de 5 s e problema (`Failed`, `Changed` ou `Removed`) |
| `app::persist` | Carga no início, recarga, conferência a cada segundo, ordem de fechamento e estado da sessão |
| `app::scripts` | Lista de consoles, scripts sem conexão, abertura, remoção e associação |
| `app::conflicts` | Pergunta sobre scripts alterados fora do SQLN |

Regras do código:

1. A carga lê os dois arquivos. Um erro de um arquivo bloqueia a aplicação inteira, sem escopo parcial.
2. Cada erro de leitura leva o arquivo e a posição (linha e coluna). A mensagem nunca leva um valor do arquivo.
3. A gravação de `config.json` toma `locks/config.lock`, lê o arquivo de novo, aplica a mudança e grava com o hash lido. O arquivo de outro escopo não muda.
4. O `config.json` guarda só o que o escopo declara. A leitura não reescreve os valores padrão.
5. O `id` de um script é um UUID e é o nome dos dois arquivos. A lista ignora um `.json` ilegível ou cujo nome difere do `id`.
6. O bloqueio de um console é o arquivo `locks/<id>.lock`, com `flock`. O sistema o libera ao fim do processo.
7. `ScriptFile::due` compara a revisão do editor com a última gravada. O laço principal passa o relógio, e o teste controla o tempo.
8. O fim de linha do arquivo (`LF` ou `CRLF`) fica no `ScriptFile`. O editor usa `LF` sempre.
9. `ScriptFile::poll` lê o arquivo só se o tamanho ou a data mudou, e depois compara o hash.
10. Uma definição de conexão leva a pasta base. `Definition::connector` resolve os caminhos relativos com ela no momento de abrir.

### Preparação de credenciais

`app::auth` recebe cada operação antes de enviá-la ao gerente.
A preparação cobre execuções, catálogo, colunas, páginas e testes de conexão das definições salvas.
Conectores prontos e definições locais sem chave de arquivo não vêm de configuração importada.

1. Conferir a confiança no destino e no caminho do projeto.
2. Solicitar confirmação quando necessário, antes de ler o cofre ou enviar credenciais.
3. Obter a senha conforme `credential`, ou usar autenticação sem senha para `none`.
4. Atualizar o conector do pool sem apagar texto, schema ou estado do editor.
5. Enviar somente a operação que ainda corresponde à solicitação atual.

Um cadastro local autoriza seu destino. Ele não autoriza implicitamente a reutilização de credenciais em um caminho novo de projeto.
Testar um formulário existente também não ignora essa confirmação.
A opção de salvar começa desmarcada; teste com senha digitada não substitui a senha da definição salva.

A fila guarda o SQL, as opções e a revisão do editor no momento da solicitação.
Editar durante a espera não muda o SQL enviado nem atribui suas marcas ao texto novo.
Cada resposta tem um token. A conexão tem uma geração, e o teste tem uma cópia do formulário.
Cancelamento ou mudança invalida respostas antigas.
Cancelar também remove os pedidos da mesma geração que aguardam a mesma preparação.

`Manager::spawn_blocking` limita as operações de sistema a uma por vez, com um semáforo assíncrono.
Trabalhos aguardam sem ocupar threads bloqueantes; um token impede que um trabalho cancelado inicie acesso ao cofre.
As respostas usam um canal privado de preparação, separado dos eventos de execução SQL.
Uma chamada nativa já iniciada pode continuar após cancelar, mas sua resposta não inicia SQL.

A interface oferece nova tentativa explícita ou senha de sessão após falha do cofre.
Falha ao gravar a referência depois de salvar no cofre mantém a credencial e informa o estado incompleto.
Sem cofre, a leitura usa `credentials.json`. Uma falha de gravação sem cofre oferece texto simples depois do aviso.
Uma recusa de senha limpa a sessão e abre uma nova solicitação. A nova senha passa por `test_connection` antes de qualquer gravação.

### Aba de tabela

Cada aba de tabela tem uma tarefa de console do gerente de execução, mas sem editor e sem transação.
A aba pede uma página com `Run`, e recebe os mesmos eventos de um console.
Assim, o prazo, a parada, o cancelamento, a fila do pool e o orçamento valem sem código novo.
A tarefa guarda a conexão somente durante a consulta, pois uma consulta de tabela nunca deixa uma transação aberta.

1. `table::page_sql` monta a consulta: `SELECT *`, `WHERE`, `ORDER BY` e `LIMIT n+1 OFFSET k`, cada cláusula em uma linha.
2. O tokenizador recusa o texto com um segundo comando ou com um token aberto, antes de enviar a consulta.
3. `RunOptions::row_cap` limita o `ResultSink` a `n+1` linhas, e o `LIMIT` do SQL garante esse teto. O sink nunca pede o cancelamento.
4. A aba guarda as `n` primeiras linhas e descarta a linha extra, com o custo dela devolvido ao orçamento. A linha extra prova que existe uma próxima página.
5. O `OFFSET` de **Carregar mais** é o número de linhas já carregadas.
6. A primeira página de uma consulta limpa as linhas, e o `ExecutionId` descarta as respostas antigas.

Uma consulta de tabela e uma consulta de console compartilham o mesmo `ExecEvent`.
A identidade da aba vem do contador de consoles do gerente, então os números não colidem.

Uma aba de chave Redis usa o mesmo caminho, sem SQL:

1. `table::page_sql` monta `SQLN.VALUE <banco> <chave> <offset> <quantidade>` por `connector::redis::value_page`. Esse texto não é um comando do servidor.
2. A sessão lê `TYPE` em cada página, porque a chave pode mudar entre duas páginas.
3. A sessão usa o comando do tipo: `GET`, `LRANGE`, `HSCAN`, `SSCAN`, `ZRANGE` ou `XRANGE`.
4. `Capabilities::table_filters` falso tira `WHERE`, `ORDER BY`, **Clear** e a ordenação da aba. **Apply** vira **Reload**.

### Limite conhecido do editor

Cada entrada de desfazer do EdTUI guarda uma cópia inteira do texto.
Por cálculo, um script de 50.000 linhas ocupa cerca de 15 MB por entrada e 1,5 GB nas 100 entradas.
Scripts comuns ficam muito abaixo disso.

**Definido:** limitar o histórico a 100 entradas e 64 MiB por console.
Remover as entradas mais antigas ao atingir qualquer limite, sem limitar o tamanho do texto.
Uma entrada isolada maior que o orçamento não entra no histórico, e a aplicação mostra um aviso.

**Definido:** `Stack` e os campos `undo` e `redo` do EdTUI são privados, e a API pública não mede nem remove entradas.
O limite fica em um fork pequeno de `Stack`, em `vendor/edtui`, por `[patch]`. `vendor/edtui/FORK.md` lista as mudanças.
`cargo install sqln` ignora `[patch]`. No lançamento, a mudança precisa estar no projeto original ou em um fork publicado com outro nome.

## 3. Concorrência

### Regras

1. A thread principal recebe eventos do terminal e eventos do runtime.
2. A atualização do estado não faz entrada ou saída.
3. Cada console tem um ator com canal de comandos, e esse ator guarda a sessão reservada.
4. O cancelamento usa um objeto separado da conexão, pois o ator pode estar ocupado.
5. O `rusqlite` roda em uma thread própria, porque é síncrono.
6. Cada lote de linhas segue para a interface por um canal limitado.

### Laço principal

A thread principal espera uma tecla por até 20 ms e depois lê os eventos da execução.
Ela lê no máximo 64 eventos por quadro, então uma carga grande não atrasa as teclas.
A busca na grade avança 8 ms por volta do laço, sem espera enquanto houver linhas para ler.
O canal de eventos é limitado, e isso controla a velocidade da leitura do banco.
A aplicação desenha somente depois de uma mudança.

### Por que usar um ator por conexão

`mysql_async` e `rusqlite` exigem acesso exclusivo à conexão durante uma instrução.
O ator mantém a leitura ativa e continua a consumir linhas depois de um cancelamento.
Os testes mostraram que abandonar a leitura pode bloquear o servidor.

### Eventos antigos

Cada execução recebe um `ExecutionId`.
A aplicação descarta um evento cujo `ExecutionId` não seja o atual da aba.
Isso impede que uma resposta antiga repovoe uma grade.

## 4. Contrato dos conectores

```rust
trait Connector: Send + Sync {
    fn dialect(&self) -> Dialect;
    fn capabilities(&self) -> Capabilities;
    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError>;
}

trait Session: Send {
    fn canceller(&self) -> Arc<dyn Canceller>;
    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError>;
    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError>;
    async fn transaction_state(&mut self) -> TransactionState;
    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError>;
    async fn reset(&mut self) -> ResetOutcome;
    async fn is_alive(&mut self) -> bool;
    async fn read_catalog(&mut self) -> Result<Catalog, ExecError>;
    async fn read_columns(&mut self, schema: &str, relation: &str) -> Result<Vec<String>, ExecError>;
}
```

O crate `async-trait` permite usar `Box<dyn Session>`.
`Capabilities` declara se a fonte tem transações, se o console escolhe um schema ou um banco e se a aba de tabela aceita filtro e ordem em SQL.
Cancelamento e paginação entram nessa declaração quando uma fonte precisar, pois os conectores usam o mesmo contrato nesses pontos.
Cada console tem uma tarefa Tokio, que guarda a sessão entre execuções enquanto a transação continuar aberta.
A tarefa mantém o future de `run` ativo depois de um cancelamento, para ler a resposta do banco.
Fechar a conexão é o único caso que abandona esse future.

| Banco | Cancelamento | Limpeza | Estado da transação |
|---|---|---|---|
| PostgreSQL | `CancelToken` | `DISCARD ALL` | Sonda por relógio, pelo protocolo simples |
| MySQL | `KILL QUERY` em conexão temporária | `Conn::reset` e volta ao banco da conexão | `SERVER_STATUS_IN_TRANS` da última resposta; `DO 0` depois de um erro |
| SQLite | `InterruptHandle` | Descartar a conexão | `is_autocommit` |
| Redis/Valkey | `CLIENT UNBLOCK` em conexão temporária; sem efeito, fecha a sessão | `DISCARD`, `UNWATCH` e volta ao banco da conexão | Sempre `None`: a fonte não tem transação manual |

`run` envia uma instrução, entrega as linhas a `ResultSink` e obedece ao retorno `Flow::Stop`.
Uma instrução pode entregar vários resultados, como `CALL` no MySQL, e o teto vale para cada um.
`ResultSink` envia `ResultDone` no fim de cada resultado.
Ao descartar `ResultSink`, as linhas ainda no lote voltam ao orçamento.
`ResultSink` aplica o teto e o orçamento, e reúne as linhas em lotes de 200.
`Value` guarda `Null`, `Text`, `Number` ou `Binary`, sempre com o texto do banco.
`Conn::reset` mantém o banco atual, então um `USE` continuaria na sessão sem o passo seguinte.
A limpeza lê `DATABASE()` e volta ao banco da conexão. Sem banco na conexão, a sessão fecha, pois o MySQL não sai de um banco.
A variante serve somente à apresentação, como alinhar números.

## 5. Execução de um script

1. O console pede a execução e recebe um `ExecutionId`.
2. A aplicação limpa os resultados anteriores e libera o orçamento.
3. O editor entrega uma cópia do texto.
4. `sql` separa as instruções e guarda suas posições.
5. `pool` entrega uma reserva, ou a execução entra na fila.
6. O ator aplica o banco ou schema selecionado.
7. Para cada instrução, o ator executa `run`.
8. Depois de cada instrução, o ator consulta o estado da transação.
9. Em erro, timeout ou parada, a sequência termina.
10. Sem transação aberta, `pool` limpa e devolve a conexão.

### Teto de linhas

1. O ator entrega linhas até o teto, mais uma linha para saber se existe mais.
2. Em uma consulta de leitura sem transação, o ator aciona o cancelamento. `sql::is_plain_read` decide pelo texto.
3. Nos demais casos, o ator consome o restante sem guardar as linhas.
4. Nos dois casos, o ator lê até a resposta da instrução.
5. Um cancelamento causado pelo teto não vira erro da instrução.

### Interrupção

1. `exec` aciona `Canceller` e inicia o prazo de 5 segundos.
2. O ator continua lendo e descartando as linhas. `Flow::Discard` indica esse estado.
3. Com a resposta, o ator espera o pedido de cancelamento terminar, e a conexão segue o fluxo normal.
4. Sem resposta e sem transação, `exec` fecha a conexão.
5. Sem resposta e com transação, a interface pede confirmação.
6. Se o pedido falhar, o prazo termina de imediato.
7. Se o pedido não terminar depois da resposta, a sessão é descartada, pois ele poderia atingir a próxima instrução.

`Canceller::cancel` retorna depois de o banco tratar o pedido.
No PostgreSQL, isso exige ler até o servidor fechar a conexão do pedido.
No MySQL, a resposta de `KILL QUERY` já confirma o pedido.

## 6. Pool próprio

Cada definição de conexão tem um pool, com este estado:

| Campo | Conteúdo |
|---|---|
| `limit` | Limite configurado, com padrão de cinco |
| `idle` | Conexões livres |
| `in_use` | Conexões reservadas |
| `opening` | Conexões em estabelecimento |
| `cleaning` | Conexões em limpeza |
| `waiters` | Fila em ordem de chegada |

A soma de `idle`, `in_use`, `opening` e `cleaning` nunca ultrapassa `limit`.
A conexão temporária de cancelamento fica fora dessa soma.

1. Com uma conexão livre, entregar essa conexão.
2. Abaixo do limite, abrir uma conexão com `timeout` de 30 segundos em toda a operação.
3. No limite, colocar a solicitação na fila, sem prazo.
4. Ao liberar uma vaga, atender a primeira solicitação ainda ativa.
5. Em erro de conexão, devolver o erro à solicitação, sem repetir a tentativa.
6. Cancelar uma espera remove a solicitação da fila.
7. Ao reutilizar uma conexão ociosa, chamar `is_alive`. Uma conexão perdida dá lugar a uma nova.

Uma sessão reservada não recebe `apply_context` de novo, pois uma transação com erro recusa `SET`.
Uma sessão com estado desconhecido é descartada.
**Desconectar** fecha as sessões ociosas e aumenta a geração do pool.
Uma sessão de uma geração anterior fecha ao voltar, como a de uma leitura do catálogo em andamento.
O evento `Disconnected` espera essa sessão por até 5 segundos, e a interface informa quando ela continua em uso.

Uma conexão com a limpeza não confirmada é descartada.
A reposição ocorre somente quando houver nova necessidade.

## 7. Estados

| Dimensão | Valores |
|---|---|
| Execução | Ociosa, aguardando conexão, executando, finalizando, concluída, teto atingido, erro, timeout, parada, cancelada antes de iniciar, interrompida para fechamento |
| Modo de transação | Autocommit ou manual |
| Transação | Inexistente, aberta, com erro ou desconhecida |
| Reserva | Sem conexão, aguardando, em uso ou em limpeza |

A interface recusa **Desconectar** e a remoção antes de enviar o comando, enquanto um console da conexão tiver execução ou transação.

| Comando | Evento de resposta |
|---|---|
| `Run` | `Queued`, `Started`, `Rows`, `StatementFinished`, `ExecutionFinished` |
| `Stop` | `Stopping`, `ConfirmClose`, `ExecutionFinished` |
| `Commit`, `Rollback` | `TransactionChanged` ou `TransactionFailed` |
| `CancelWait` | `ExecutionFinished` |
| `Disconnect` | `Disconnected` |
| `TestConnection` | `ConnectionTested` |

## 8. Arquivos

### `config.json`

Este esquema vale para os escopos global e de projeto.
Nenhum escopo recebe `password`. A senha autorizada em texto simples fica em `credentials.json`.

```json
{
  "version": 1,
  "preferences": {
    "keyboardMode": "conventional",
    "pageSize": 100,
    "rowCap": 1000,
    "gridBudgetBytes": 268435456,
    "keymap": {},
    "theme": "auto",
    "icons": "unicode",
    "reopenTabs": true,
    "suggestionKeys": "tab"
  },
  "connections": [
    {
      "id": "0f3726f5-16d2-46af-82f9-54736aaa81f8",
      "name": "app",
      "kind": "postgres",
      "host": "db.example.test",
      "port": 5432,
      "database": "app_db",
      "username": "app_user",
      "credential": "vault",
      "readOnly": false,
      "tls": { "mode": "verify", "caFile": null, "clientCert": null, "clientKey": null },
      "pool": { "maxConnections": 5 },
      "connectTimeoutSeconds": 30,
      "sqlTimeoutSeconds": 30,
      "theme": "nord"
    },
    {
      "id": "6a1d4c12-7c0e-4f0b-9a55-0c2f4b6f7d10",
      "name": "local",
      "kind": "sqlite",
      "file": "data/app.db",
      "foreignKeys": false,
      "busyTimeoutMillis": 5000,
      "readOnly": false,
      "pool": { "maxConnections": 5 },
      "sqlTimeoutSeconds": 30
    }
  ]
}
```

| Campo | Valores |
|---|---|
| `kind` | `postgres`, `mysql`, `sqlite` ou `redis` |
| `credential` | `vault`, `session`, `globalPlainText` ou `none` |
| `tls.mode` | `verify` ou `disabled` |
| `theme` | Opcional. Um valor de `theme` de `preferences`. Sem o campo, a conexão usa o tema padrão |
| `icon` | Opcional. `warning`, `skull`, `radiation`, `flag`, `crown`, `flask`, `star`, `diamond`, `tools`, `gear`, `home`, `heart`, `snowflake` ou `check`. Sem o campo, a conexão usa o ícone do banco |

O valor `globalPlainText` existe somente no escopo global.
O valor `sqlTimeoutSeconds: null` executa sem prazo, e a ausência do campo vale 30 segundos.

Regras da leitura, conforme o código:

- `version` é obrigatório e vale 1. Um campo desconhecido invalida o arquivo.
- Todos os campos de `preferences` são opcionais, e `keymap` tem as seções `conventional` e `vim`.
- `preferences.export` guarda as escolhas do modal de exportação: `format`, `header`, `delimiter`, `nullText`, `bom`, `grouped`, `batch` e `folder`. A aplicação grava esse campo ao confirmar uma exportação, e ele só é válido no arquivo global.
- `credential` ausente vale `none`. `pool` ausente vale 5 conexões.
- Um `id` ou um `name` repetido no mesmo arquivo invalida o arquivo. O nome ignora maiúsculas e minúsculas.
- Um caminho relativo (`file`, `caFile`, `clientCert`, `clientKey`) fica como o usuário o escreveu no arquivo. A resolução ocorre ao abrir a conexão.
- O formulário grava o caminho completo de um arquivo SQLite digitado, a partir do diretório atual. Um `file` salvo e não alterado mantém o seu texto.
- Uma conexão de rede sem `tls` usa TLS com verificação, exceto em `localhost`, loopback e socket Unix.
`apply_context` reaplica `readOnly` a cada reserva, pois a limpeza da sessão o remove.

### Metadados do console

Cada console tem `consoles/<id>.sql` e `consoles/<id>.json`:

```json
{
  "version": 1,
  "id": "9b2e7d44-3f6a-4c55-8e21-1d4f0a7c9e33",
  "name": "investigar-pedidos",
  "connectionId": "0f3726f5-16d2-46af-82f9-54736aaa81f8",
  "connectionScope": "project",
  "schema": "public"
}
```

### Estado local

`workspace.json` nunca entra no Git.
**Definido:** se o arquivo estiver ilegível, usar o estado inicial e mostrar um aviso não bloqueante.
O código ainda usa o estado inicial sem aviso.

Exemplo:

```json
{
  "version": 1,
  "consoles": {
    "9b2e7d44-3f6a-4c55-8e21-1d4f0a7c9e33": {
      "cursor": { "row": 3, "col": 5 },
      "scroll": { "row": 0, "col": 0 }
    }
  },
  "layout": { "navigatorCollapsed": false, "navigatorWidth": 24, "editorRatio": 0.5 },
  "openTabs": {
    "tabs": [
      { "kind": "console", "scope": "project", "id": "018f…" },
      { "kind": "table", "scope": "global", "connection": "g1", "schema": "public", "relation": "users", "view": false }
    ],
    "active": 0
  }
}
```

### Lista de arquivos

| Arquivo | Local | Git |
|---|---|---|
| `config.json` | Global e projeto | Versionável no projeto |
| `consoles/<id>.sql` | Global e projeto | Versionável no projeto |
| `consoles/<id>.json` | Global e projeto | Versionável no projeto |
| `workspace.json` | Global e projeto | Ignorado |
| `locks/<id>.lock` | Global e projeto | Ignorado |
| `.gitignore` | Projeto | Versionável |
| `trust.json` | Somente global | Fora do projeto |
| `credentials.json` | Somente global | Fora do projeto |

Conteúdo de `.sqln/.gitignore`:

```text
workspace.json
locks/
*.tmp
```

`trust.json` guarda os destinos confirmados e os caminhos conhecidos dos projetos.
O formato implementado tem `version: 1`, `destinations` e `projects`.
Cada destino inclui motor, endereço TCP ou socket, usuário e configuração TLS, sem senha.
Cada projeto registra `connectionId` e uma lista de `paths` canônicos.
O caminho não participa da conta do cofre; um caminho novo exige confirmação separada.
`Trust::required` não cria arquivos. Arquivo inválido ou ilegível retorna erro, sem autorizar o destino.
`Trust::confirm` toma `locks/trust.lock`, relê o arquivo e grava com conferência do hash.
O arquivo e seu temporário têm permissão `0600` no Unix.
Nenhuma chamada grava confiança nos arquivos do projeto.

`credentials.json` guarda as senhas autorizadas em texto simples.
O arquivo fica na pasta global e tem permissão `0600` no Unix, inclusive no temporário.
`PlainFile` recusa um link, um arquivo com permissão aberta e um conteúdo inválido, sem sobrescrever.
Cada entrada usa como chave a conta da seção 9, para conexões globais e de projeto.
Sem cofre, a aplicação lê esse arquivo pela identidade antes de pedir a senha.
Mudar o destino, o usuário ou o TLS muda a chave, então a senha anterior não é enviada.

## 9. Referência de credencial

No cofre, cada entrada usa o serviço `sqln` e uma conta com este formato:

```text
<escopo>/<id-da-conexão>/<motor>/<endereço-JSON-escapado>/<usuário-escapado>/<TLS-JSON-escapado>
```

`Identity::account` implementa esse formato com escape percentual por componente.
O motor também participa da identidade, para separar PostgreSQL de MySQL.
O endereço inclui host e porta, ou o caminho do socket.
O TLS inclui modo e caminhos configurados de CA, certificado de cliente e chave.
Usar os caminhos como configurados, sem incluir a raiz do projeto em caminhos relativos.
A confirmação separada do caminho do projeto controla a reutilização após mover ou copiar a pasta.
O escape impede colisões entre separadores, percentuais e componentes com texto arbitrário.

Mudar o destino, o usuário ou o TLS muda a conta.
Assim, a aplicação não encontra a senha anterior e precisa pedir outra.
Renomear a conexão não muda a conta.
O caminho da pasta não faz parte da conta.

**Definido:** usar `keyring-core` com um armazenamento específico por plataforma.
Usar Keychain no macOS e Secret Service no Linux.
A documentação do `keyring` 4 recomenda esse caminho para aplicações.
`SystemVault` já oferece leitura, gravação e exclusão com um armazenamento próprio, sem alterar o padrão global do processo.
`Credentials` guarda senhas de sessão por `Identity` e mantém a senha em memória após falha de gravação.
`save_verified` grava e confere a leitura; ele não remove nenhuma fonte em texto simples.
Erros do cofre usam mensagens fixas, sem o texto do provedor nem bytes de senha.
`app::auth` já usa esses adaptadores. Os testes automáticos injetam `Options::vault`, sem acessar o cofre real.

A tabela abaixo descreve as respostas reais, conforme a [validação do plano](../PLAN.md#validação-dos-cofres-reais).

| Resposta real | Sistema | Resultado | Ação |
|---|---|---|---|
| `NoEntry` | macOS e Linux | Sem credencial | Pedir a senha |
| Socket do barramento inexistente | Linux | `Absent` | Aplicar as regras de cofre ausente |
| `ServiceUnknown` do D-Bus | Linux | `Absent` | Aplicar as regras de cofre ausente |
| `secret_service::Error::Unavailable` | Linux | `Absent` | Aplicar as regras de cofre ausente |
| `Locked`, `NoResult` e `Prompt` | Linux | `NoAccess` | Oferecer nova tentativa e senha de sessão |
| `-25244` | macOS | `NoAccess` | Oferecer nova tentativa e senha de sessão |
| `-128`, `-25293`, `-25299`, `-25307` e `-25308` | macOS | `Failed` | Oferecer nova tentativa e senha de sessão |
| Falha desconhecida | macOS e Linux | `Failed`, nunca `Absent` | Oferecer nova tentativa e senha de sessão |

`is_absent` aceita somente as três respostas `Absent`, pelo tipo do erro.
No macOS, o Keychain faz parte do sistema, então nenhuma falha significa cofre ausente.
`SystemVault` usa um armazenamento próprio, então `NoDefaultStore` não ocorre nesse caminho.
O Keychain bloqueado retorna `-25293` ou `-128`, não `NoStorageAccess`, e a interface oferece as mesmas opções.

Com falta de acesso ou erro, oferecer **Tentar novamente**, **Usar senha nesta sessão** e **Cancelar**.
Solicitar a senha antes do primeiro uso que precisar dela, inclusive teste e leitura do catálogo.
Iniciar a solicitação com uso somente na sessão; salvar exige uma escolha explícita.
A confirmação de confiança mostra destino, usuário e TLS, com **Cancelar** como foco inicial.
Os testes reais confirmam esses critérios no Keychain e no Secret Service em contêiner.
Repetir o Secret Service em um desktop Linux real antes do lançamento.

### Senha recusada

`ConnectError::password_refused` marca somente a recusa do banco: `28P01` no PostgreSQL, `1045` e `1698` no MySQL.
Uma mensagem de erro não comprova a recusa, e `28000` também cobre uma regra de `pg_hba.conf`.
`ExecEvent::PasswordRefused` traz o número do pool. Um pool antigo não fala sobre a senha atual.
Várias sessões podem falhar com a mesma senha, então somente o primeiro evento conta.

| Estado | Comportamento |
|---|---|
| Primeira recusa | Abrir a solicitação. A nova senha passa por `test_connection` antes da gravação |
| Segunda recusa | Suspender a definição e mostrar o aviso |
| Durante a suspensão | Catálogo e colunas não abrem sessão; cada ação explícita abre a solicitação |
| Senha aceita ou edição da conexão | Encerrar a suspensão. A ação pendente continua, pois nenhum SQL chegou ao banco |

Uma falha de rede ou TLS durante a verificação mostra o erro e não conta como recusa.
Com sessões ainda abertas, a senha nova só entra depois de desconectar.

## 10. Fluxos de recuperação

### Gravação de arquivo

1. Gravar o temporário na mesma pasta. Para `config.json`, `trust.json` e `credentials.json`, usar nome exclusivo e permissão `0600` no Unix.
2. Sincronizar o arquivo com o disco.
3. Comparar o hash do arquivo de destino com o último hash conhecido.
4. Renomear o arquivo temporário sobre o destino.
5. Em caso de falha, remover o temporário, manter o texto e mostrar o alerta.

### Encerramento inesperado

1. Ao iniciar, remover os arquivos `.tmp` antigos.
2. O sistema libera os bloqueios do processo encerrado.
3. O banco encerra as sessões e as transações abertas, conforme sua semântica.
4. A aplicação não tenta recuperar transações nem executa SQL.

O salvamento pode perder até 500 ms de digitação.

### Falha ao gravar a senha

1. Manter a senha em memória.
2. Informar que a senha não foi salva.
3. Oferecer nova tentativa explícita.

### Migração para o cofre

1. Detectar uma senha global com um cofre disponível.
2. Pedir confirmação.
3. Gravar a senha no cofre.
4. Ler a senha do cofre e comparar.
5. Oferecer a remoção da senha global.
6. Em qualquer falha, manter a senha global e informar a migração incompleta.

### Associação de um script sem conexão

1. O usuário escolhe uma conexão.
2. Se o escopo mudar, a interface mostra a nova localização.
3. O usuário confirma.
4. Copiar o `.sql` e o `.json` para o destino.
5. Verificar os dois arquivos.
6. Remover os originais.
7. Atualizar `connectionId` e `connectionScope`.

A aplicação não executa SQL nesse fluxo.

### Projeto movido ou copiado

1. Comparar a raiz do projeto com os caminhos de `trust.json`.
2. Se o caminho for novo, pedir confirmação antes de reutilizar uma credencial.
3. Registrar o novo caminho depois da confirmação.

### Perda de conexão

1. Conexão ociosa: remover do pool, sem mensagem.
2. Conexão em execução: falhar a instrução e não repetir o SQL.
3. Transação aberta: marcar o estado como desconhecido.
4. Antes da próxima execução nesse console, exigir confirmação.
