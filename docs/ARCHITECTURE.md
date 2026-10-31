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
| `ui` | Desenho das telas e dos widgets |
| `input` | Mapa de teclas: regiões, ações, sequências e regras de `keymap` |
| `navigator` | Árvore de conexões, schemas, relações e colunas |
| `form` | Campos e validação do cadastro de conexão |
| `connection` | Definições de conexão e o estado de cada conexão na aplicação |
| `editor` | EdTUI, camada convencional, busca e substituição |
| `sql` | Tokenizador, separação e classes de instruções |
| `complete` | Sugestões do autocomplete |
| `catalog` | Cache em memória; schemas e tabelas primeiro, colunas sob demanda |
| `console` | Editor, resultados, mensagens e estado de um console |
| `exec` | Execuções, scripts, teto e cancelamento |
| `pool` | Limite, fila, reserva e limpeza |
| `connector` | Contrato e conectores PostgreSQL, MySQL e SQLite |
| `grid` | Linhas, larguras, busca, orçamento e TSV |
| `find` | Comparação de texto das duas buscas |
| `clipboard` | Área de transferência do sistema e OSC 52 |
| `terminal` | Preparação e restauração do terminal |
| `store` | Configuração, consoles, salvamento e bloqueios |
| `creds` | Cofre e senhas de sessão, com `zeroize` |
| `trust` | Destinos confirmados e caminhos conhecidos |

`ui` e `input` não dependem de `connector`.
Somente `connector` conhece tipos dos drivers.

`store`, `creds` e `trust` ainda não existem.
`app` aplica a prioridade das teclas e o mouse. `app/keys.rs`, `app/close.rs`, `app/connections.rs` e `app/mouse.rs` dividem esse código.
As definições de conexão ficam em memória até a etapa de configuração.

### Limite conhecido do editor

Cada entrada de desfazer do EdTUI guarda uma cópia inteira do texto.
Por cálculo, um script de 50.000 linhas ocupa cerca de 15 MB por entrada e 1,5 GB nas 100 entradas.
Scripts comuns ficam muito abaixo disso.

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
`Capabilities` declara se a fonte tem transações e se o console escolhe um schema ou um banco.
Cancelamento e paginação entram nessa declaração quando uma fonte diferente chegar, pois os três conectores são iguais nesses pontos.
Cada console tem uma tarefa Tokio, que guarda a sessão entre execuções enquanto a transação continuar aberta.
A tarefa mantém o future de `run` ativo depois de um cancelamento, para ler a resposta do banco.
Fechar a conexão é o único caso que abandona esse future.

| Banco | Cancelamento | Limpeza | Estado da transação |
|---|---|---|---|
| PostgreSQL | `CancelToken` | `DISCARD ALL` | Sonda por relógio, pelo protocolo simples |
| MySQL | `KILL QUERY` em conexão temporária | `Conn::reset` e volta ao banco da conexão | `SERVER_STATUS_IN_TRANS` da última resposta; `DO 0` depois de um erro |
| SQLite | `InterruptHandle` | Descartar a conexão | `is_autocommit` |

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
O projeto nunca recebe `password`.

```json
{
  "version": 1,
  "preferences": {
    "keyboardMode": "conventional",
    "pageSize": 100,
    "rowCap": 1000,
    "gridBudgetBytes": 268435456,
    "keymap": {}
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
      "sqlTimeoutSeconds": 30
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
| `kind` | `postgres`, `mysql` ou `sqlite` |
| `credential` | `vault`, `session`, `globalPlainText` ou `none` |
| `tls.mode` | `verify` ou `disabled` |

O valor `globalPlainText` existe somente no escopo global.
O valor `sqlTimeoutSeconds: null` executa sem prazo.
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

`workspace.json` nunca entra no Git:

```json
{
  "version": 1,
  "consoles": {
    "9b2e7d44-3f6a-4c55-8e21-1d4f0a7c9e33": {
      "cursor": { "row": 3, "col": 5 },
      "scroll": { "row": 0, "col": 0 }
    }
  },
  "layout": { "navigatorCollapsed": false, "navigatorWidth": 24, "editorRatio": 0.5 }
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

Conteúdo de `.sqln/.gitignore`:

```text
workspace.json
locks/
*.tmp
```

`trust.json` guarda os destinos confirmados e os caminhos conhecidos dos projetos.

## 9. Referência de credencial

No cofre, cada entrada usa o serviço `sqln` e uma conta com este formato:

```text
<escopo>/<id-da-conexão>/<host>:<porta>/<usuário>/<tls>
```

Exemplo: `project/0f3726f5-16d2-46af-82f9-54736aaa81f8/db.example.test:5432/app_user/verify`.

Mudar o destino, o usuário ou o TLS muda a conta.
Assim, a aplicação não encontra a senha anterior e precisa pedir outra.
Renomear a conexão não muda a conta.
O caminho da pasta não faz parte da conta.

Para o projeto, usar `keyring-core` com um armazenamento específico por plataforma.
A documentação do `keyring` 4 recomenda esse caminho para aplicações.

| Resposta do cofre | Significado | Ação |
|---|---|---|
| `NoEntry` | Sem credencial | Pedir a senha |
| `NoStorageAccess` | Cofre bloqueado ou sem acesso | Oferecer nova tentativa e senha de sessão |
| `NoDefaultStore` | Cofre ausente | Aplicar as regras de cofre ausente |
| `PlatformFailure` | Erro do cofre | Oferecer nova tentativa e senha de sessão |

O bloqueio do Keychain e o cofre do Linux ainda não foram testados.

## 10. Fluxos de recuperação

### Gravação de arquivo

1. Gravar em `<arquivo>.tmp`, na mesma pasta.
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
