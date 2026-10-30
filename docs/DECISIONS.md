# SQLN — Registro de decisões

Este documento registra as escolhas e as alternativas rejeitadas.
As regras completas estão no [plano](../PLAN.md).
Não reabrir uma decisão sem um fato novo.

## Execução e resultados

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Resultado no console | Ler até um teto e encerrar a leitura | Leitura aberta com pausa | Uma pausa encerrou a conexão no MySQL e prende recursos |
| Mais linhas no console | O usuário aumenta o teto e executa de novo | Nova consulta por página | Repete comandos, altera o SQL e pode mostrar outros dados |
| Teto padrão | 1.000 linhas | 500 linhas | Valor do lazysql; Harlequin e DataGrip usam 500 |
| Fim da leitura no teto | Conforme o comando | Sempre cancelar ou sempre consumir | Cancelar pode desfazer uma alteração |
| Visão de tabela | Nova consulta por página | Leitura aberta entre páginas | Libera a conexão; aceita-se o risco de ordem instável |
| Próxima página | Pedir uma linha a mais | `COUNT` | Evita custo no banco |
| Aba em leitura | Bloquear o fechamento | Confirmar ou descartar | Menos estados |
| Valores da grade | Texto do banco; tipo só na apresentação | Decodificar tipos em Rust | Preserva precisão e aceita tipos desconhecidos |
| Parar uma instrução | Cancelar e fechar como último recurso | Só fechar a conexão | Com a conexão fechada, um `INSERT` terminou nos dois bancos |
| Prazo do cancelamento | 5 segundos | Sem prazo | DataGrip oferece escalonamento parecido |
| Conexão de cancelamento | Temporária e fora do limite do pool | Respeitar o limite literalmente | O protocolo exige outra conexão |

## Sessão e conexão

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Estado de sessão | Seletor de banco ou schema por console | Conexão fixa por console | Cinco consoles ocupariam o pool |
| Conectar | No primeiro uso | Ação explícita ou ao iniciar | Pede a senha somente quando necessário |
| Queda de conexão | Substituir ociosas e falhar reservadas | Repetir o comando | Repetir pode duplicar alterações |
| Limpeza falha | Descartar a conexão | Recuperar | Mais simples e seguro |
| Fila do pool | Sem limite | Com limite | Cada console tem uma execução |
| Confiança | Confirmar por destino | Confirmar por projeto | Segue o modelo do SSH e não depende de detectar clones |
| Somente leitura | Campo por conexão, aplicado pelo banco | Confirmar `DELETE` sem `WHERE` | O banco aplica a regra |
| Filtro da tabela | Uma única instrução | Aceitar qualquer texto | Os drivers aceitam várias instruções |

## Bibliotecas

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Drivers | `tokio-postgres`, `mysql_async` e `rusqlite` | SQLx | `timeout` falhou, faltam cancelamento e reset, e o pool escondeu conexão recusada |
| Pool | Implementação própria | Pool de biblioteca | Regras próprias de fila, reserva e limpeza |
| Editor | EdTUI com camada convencional | `ratatui-textarea` | Falta realce de sintaxe e modo Vim |
| Análise de SQL | Tokenizador próprio | `sqlparser-rs` ou tree-sitter | Precisa tolerar texto incompleto; protótipo aprovado |
| Sonda no PostgreSQL | Relógio pelo protocolo simples | `SAVEPOINT` | `SAVEPOINT` gera erros no log |
| Sonda no MySQL | `SET TRANSACTION`, preservando o modo | `performance_schema` | Exige privilégios |

## Produto

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Entrega | Primeira versão completa, com versionamento local | Versões parciais públicas | Decisão do projeto |
| TSV | Formato do DataGrip, menos as perdas de dados | Cópia exata do DataGrip | A cópia não deve perder dados em silêncio |
| Atalhos no Mac | Sem depender de `Ctrl`+setas, `Home`, `End`, `PageUp` e `PageDown` | Atalhos clássicos de IDE | Parte não funciona, e muitos teclados não têm essas teclas |
| Prefixo | `Ctrl+W` nos dois modos | Fechar aba com `Ctrl+W` | Caminho comum, sem depender do terminal |
| Commit por atalho | Sem confirmação | Pedir confirmação | Mesmo comportamento dos botões |
| Metadados | Um `.json` ao lado de cada `.sql` | Índice único ou comentário no script | Reduz conflitos no Git e mantém o script limpo |
| Telemetria | Nenhuma | Opcional | A aplicação só acessa a rede para falar com os bancos |
| Arquivo `.sql` externo | Depois da primeira versão | Na primeira versão | Reduz o escopo |
| Licença | `MIT OR Apache-2.0` | GPL ou proprietária | Padrão do ecossistema Rust |
