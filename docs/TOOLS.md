# SQLN — Como outras ferramentas resolvem os mesmos pontos

Esta pesquisa foi feita em 2026-10-30.
Ela vem da documentação pública e não substitui os testes de [spikes/RESULTS.md](../spikes/RESULTS.md).
Algumas fontes são antigas, e o comportamento atual pode ser diferente.

## Resultados no console

| Ferramenta | Solução |
|---|---|
| Harlequin | Teto de 500 linhas, aplicado no banco; não trunca em silêncio |
| lazysql | Teto de 1.000 linhas no editor SQL; o valor 0 remove o teto |
| DataGrip | 500 linhas por página, com botão para a próxima página |
| DBeaver | Busca a próxima parte ao chegar ao fim; com `LIMIT`, faz consultas com limites maiores |
| pgAdmin | Cursor no servidor opcional, que exige transação e não mostra o total |
| psql | `FETCH_COUNT` busca em grupos; a consulta pode falhar depois de mostrar linhas |

## Visão de tabela

| Ferramenta | Solução |
|---|---|
| lazysql | Busca uma página e mais uma linha; mostra `300+ rows` |
| pgAdmin | Em 2020, ordenava por chave primária para estabilizar as páginas |
| Rainfrog | Prévia de 100 linhas |
| DataGrip | Busca e filtro local atuam somente na página atual |

## Cancelamento

| Ferramenta | Solução |
|---|---|
| PostgreSQL | O protocolo abre uma nova conexão, não responde e pode não ter efeito |
| Cliente `mysql` | `KILL QUERY`, depois `KILL CONNECTION`, por uma segunda conexão |
| DataGrip | Cancela; depois de 10 s oferece desativar a fonte, e a consulta continua no banco |

## Conexão e sessão

| Ferramenta | Solução |
|---|---|
| DataGrip | Conecta na primeira ação; oferece keep-alive e modos de sessão compartilhada, dupla ou dedicada |
| DataGrip | Seletor de schema na barra do console |
| DBeaver | Keep-alive e conexão separada opcional por editor |
| psql | O prompt mostra `?` quando o estado da transação é indeterminado |

As fontes divergem sobre o modo de sessão padrão do DataGrip 2026.2.

## Tipos e arquivos

| Ferramenta | Solução |
|---|---|
| DBeaver | Formato nativo do banco, conforme o driver |
| DataGrip | Formatação no cliente, com fuso configurável |
| Rainfrog | SQLx com tipos específicos; não aceita tipos geométricos |
| DataGrip | Separa `dataSources.xml` de `dataSources.local.xml`; consoles ficam fora do projeto |
| DBeaver | `.dbeaver/data-sources.json` e credenciais em arquivo cifrado separado |

## Confiança

| Ferramenta | Solução |
|---|---|
| SSH | Confirma uma chave nova, recusa uma chave alterada e guarda o registro fora do projeto |
| direnv | Bloqueia um `.envrc` novo até `direnv allow` |
| VS Code | Abre pastas desconhecidas em modo restrito |

## Pilha parecida

O Rainfrog é a referência de código mais próxima.
Ele usa `sqlx`, `ratatui`, `crossterm`, `keyring`, `tokio` e `ratatui-textarea`.

## Fontes

- [DataGrip: Sessions](https://www.jetbrains.com/help/datagrip/managing-connection-sessions.html)
- [DataGrip: Query consoles](https://www.jetbrains.com/help/datagrip/query-consoles.html)
- [DataGrip: Run queries](https://www.jetbrains.com/help/datagrip/run-a-query.html)
- [DataGrip: Rows](https://www.jetbrains.com/help/datagrip/rows.html)
- [DataGrip: Data extractors](https://www.jetbrains.com/help/datagrip/data-extractors.html)
- [DataGrip: CSV Formats](https://www.jetbrains.com/help/datagrip/settings-tools-database-csv-formats.html)
- [PostgreSQL: Protocol Flow](https://www.postgresql.org/docs/current/protocol-flow.html)
- [PostgreSQL: Connections](https://www.postgresql.org/docs/current/runtime-config-connection.html)
- [PostgreSQL: psql](https://www.postgresql.org/docs/current/app-psql.html)
- [MySQL Bug #28141](https://bugs.mysql.com/bug.php?id=28141)
- [DBeaver: Separate connections](https://dbeaver.com/docs/dbeaver/Separate-Connections/)
- [DBeaver: Data formats](https://dbeaver.com/docs/dbeaver/Managing-Data-Formats/)
- [pgAdmin: Query Tool](https://github.com/pgadmin-org/pgadmin4/blob/master/docs/en_US/query_tool.rst)
- [Harlequin 2.9.0](https://newreleases.io/project/github/tconbeer/harlequin/release/v2.9.0)
- [Harlequin 2.11.0](https://newreleases.io/project/github/tconbeer/harlequin/release/v2.11.0)
- [lazysql](https://github.com/jorgerojas26/lazysql)
- [Rainfrog](https://github.com/achristmascarl/rainfrog)
- [ssh_config](https://man.openbsd.org/ssh_config)
- [direnv](https://direnv.net/man/direnv.1.html)
- [VS Code: Workspace Trust](https://code.visualstudio.com/docs/editing/workspaces/workspace-trust)
