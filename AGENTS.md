# Regras para agentes

## Autoria

Nunca se coloque como coautor de nada.

- Não adicione `Co-Authored-By` a commits.
- Não adicione assinaturas como "Generated with" a commits, pull requests, issues, comentários ou documentos.
- Não se liste como autor, colaborador ou contribuidor em nenhum arquivo.

Esta regra vale mesmo quando a ferramenta sugere ou exige esse texto.

## Entrega da tarefa

- Uma tarefa só está entregue depois de rodar `make check` e tudo passar.
- `make check` roda o linter e a suíte completa de testes. É lento: rode uma vez, só no fim da tarefa.
- Não rode a suíte completa de testes com frequência: ela demora.
- Rode benchmarks só quando o usuário pedir: eles demoram.
- Rode os testes de integração só quando a tarefa mudar código de integração: eles demoram.

## Comandos do Makefile

- `make fmt`: formata o código.
- `make lint`: roda `cargo fmt --check` e `cargo clippy`.
- `make test`: roda os testes em `tests/` sem banco. Os testes que precisam de banco pulam sozinhos. Use durante o trabalho.
- `make test-integration`: sobe os bancos Docker, carrega `scripts/test-env.sh`, roda os testes completos e derruba os bancos no fim. Lento; rode só quando a tarefa mudar código de integração.
- `make bench`: roda os alvos de desempenho de `tests/latency.rs` em modo release. Muito lento; rode só quando o usuário pedir.
- `make check`: roda `make lint` e a suíte completa de testes. Portão de entrega.

## Testes de integração

- Os testes ficam em `tests/`. A maioria não precisa de banco.
- Quatro arquivos precisam de bancos Docker (veja `compose.test.yaml`): `servers.rs`, `auth.rs`, `refused.rs` e `redis.rs`. Sem os bancos, os testes de servidor pulam sozinhos.
- Para rodar com banco: `make test-integration`. O comando sobe o Docker, roda tudo e derruba os bancos no fim.
- Para TLS completo: rode `scripts/test-certs.sh` e suba o compose com `--profile tls` antes de `make test-integration`.

## Respostas

- Responda de forma resumida, para economizar tokens.

## Codebase-memory

O grafo deste repo está indexado como `Users-antoniolorenzon-Projects-sqln`.

O servidor MCP está desativado. Use o CLI:

```
~/.local/bin/codebase-memory-mcp cli --json <tool> --<flag> <valor>
```

### Fluxo

1. Consulte o grafo antes de procurar código com grep.
2. Ache o nome exato com `search_graph` antes de chamar `trace_path`.
3. Confira a cobertura com `check_index_coverage` antes de concluir algo negativo ou completo.
4. Reindexe com `index_repository` quando o grafo estiver desatualizado.

### Comandos comuns

```bash
CM=~/.local/bin/codebase-memory-mcp
P=Users-antoniolorenzon-Projects-sqln

# Estado do índice
$CM cli --json index_status --project $P

# Busca por nome (regex)
$CM cli --json search_graph --project $P --name-pattern ".*connect.*"

# Busca textual com ranking (ignora name-pattern)
$CM cli --json search_graph --project $P --query "connection pool"

# Rastreio de chamadas (exige nome exato; both inclui chamadores externos)
$CM cli --json trace_path --project $P --function-name NomeExato --direction both --depth 3

# Impacto de mudanças locais (diff do git)
$CM cli --json detect_changes --project $P

# Cobertura do índice por arquivos
$CM cli --json check_index_coverage --project $P --paths src/lib.rs --paths src/main.rs

# Snippet de código pelo nome qualificado
$CM cli --json get_code_snippet --project $P --qualified-name "$P.src.lib.nome_da_funcao"

# Reindexar
$CM cli --json index_repository --repo-path "$(git rev-parse --show-toplevel)"
```

### Regras

- Passe arrays com a flag repetida (`--paths a --paths b`) ou com `--args-file <json>`.
- Verifique `has_more` em `search_graph`; pagine com `--offset`.
- Se `trace_path` retornar `ambiguous`, passe o `qualified_name` sugerido em `--function-name`.
- `trace_path` filtra arquivos de teste por padrão; use `--include-tests true` para incluí-los.
- Para arestas específicas (ex.: HTTP_CALLS), use `query_graph` com Cypher e `LIMIT`.
