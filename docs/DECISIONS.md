# SQLN — Registro de decisões

Este documento registra as escolhas e as alternativas rejeitadas.
As regras completas estão no [plano](../PLAN.md).
Não reabrir uma decisão sem um fato novo.

## Fechamento das decisões — 2026-10-31

**Definido:** o usuário aceita todas as 12 recomendações abaixo.
A aprovação fecha as decisões, mas não declara as mudanças como implementadas.
A seção 1 do plano separa o código atual dos ajustes que faltam.

| Ponto | Decisão aprovada | Referência |
|---|---|---|
| 1. Renomear consoles | `Ctrl+W N` e ação na lista; recusar nome vazio ou repetido na conexão; preservar identidade e arquivos | Plano, seção 12 |
| 2. Trocar conexão | `Ctrl+W a`, confirmação, salvamento anterior, preservação do texto e limpeza dos resultados; não executar nem converter SQL | Plano, seção 12 |
| 3. CTEs | Sugerir nomes e colunas identificáveis de `WITH`, com aliases e escopo; não consultar o banco para descobri-las | Plano, seção 6 |
| 4. Desfazer | Até 100 entradas e 64 MiB por console; remover as mais antigas e avisar sobre entrada isolada acima do orçamento | Plano, seção 5 |
| 5. Dicas e ajuda | Mostrar teclas efetivas do `keymap` e somente ações disponíveis, inclusive no prefixo | Plano, seção 5 |
| 6. Configurações | `Ctrl+W ,`, escopos, valores herdados e os cinco campos do esboço; avisar antes de perder seleção ou histórico ao trocar teclado | Plano, seção 11 |
| 7. Arquivos | Aprovar o salvamento e os conflitos existentes; usar **Fechar e descartar texto** para o arquivo removido | Plano, seção 12 |
| 8. Listas | Aprovar listas de consoles e scripts sem conexão, confirmação de exclusão e associação sem execução | Plano, seção 12 |
| 9. Configuração e estado | Aprovar recarga, bloqueio por erro e escopos; avisar sem bloquear quando `workspace.json` estiver ilegível | Plano, seções 11 e 12 |
| 10. Tabelas | Aprovar abertura, rótulos, ordenação e páginas; misturar abas na ordem de abertura e não criar console ao fechar o último com tabela aberta | Plano, seção 10 |
| 11. Credenciais | Uso inicial só na sessão, gravação explícita, recuperação e migração confirmadas; confiança com foco em Cancelar | Plano, seção 13 |
| 12. Cofre | `keyring-core`, Keychain e Secret Service; falha desconhecida é erro, nunca ausência; validar em serviços reais | Plano, seção 13; arquitetura, seção 9 |

Terminal real, mouse, disco de rede, certificados e falhas de rede continuam como validações pendentes.

## Estado da entrega de credenciais

O código implementa a base e o primeiro uso, mas não conclui a etapa de credenciais.
As regras de texto simples autorizado, migração, senha recusada e exclusão separada continuam aprovadas e pendentes de implementação.
A validação em cofres reais também continua pendente.
A [matriz do plano](../PLAN.md#fechamento-da-etapa-de-credenciais) registra os critérios de conclusão.
Os testes com cofre em memória e bancos reais não substituem essa validação nativa.

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
| Aba de tabela | Tarefa de console sem editor, com uma página por `Run` | Tarefa própria para as páginas | Reaproveita prazo, parada, cancelamento, fila do pool e orçamento |
| Linha extra da página | Teto do sink em `n+1`, com `LIMIT n+1` no SQL | Teto `n`, com o sink pedindo o cancelamento na linha extra | O cancelamento abre uma conexão nova no PostgreSQL e no MySQL a cada página |
| Cláusulas da consulta de tabela | Uma por linha | Texto em uma linha | Um comentário `--` no filtro removeria o `LIMIT` |
| Carregar mais com o orçamento cheio | Recusar até algum dado sair do orçamento; um valor maior que o orçamento recusa sempre | Tentar a consulta e falhar de novo | Evita uma consulta que não pode terminar |
| Aba de tabela fechada em leitura | Parar a consulta e fechar sem pergunta | Bloquear como o resultado de um console | A consulta não tem transação, então parar não perde dados |
| Valores da grade | Texto do banco; tipo só na apresentação | Decodificar tipos em Rust | Preserva precisão e aceita tipos desconhecidos |
| Fim da leitura no SQLite | Encerrar no cliente com `readonly`; consumir nos demais casos | Sempre consumir | Resultado infinito terminaria somente pelo prazo |
| Aba depois da execução | Último resultado; **Mensagens** em qualquer outro fim | Sempre o último resultado | Uma parada ficaria invisível atrás de uma grade vazia |
| Cursor da grade | Conta como seleção de uma célula para `Ctrl+C` | Exigir seleção | A grade sempre tem uma célula atual, como no DataGrip |
| Função em consulta no teto | Consumir as linhas | Cancelar toda consulta com `SELECT` | O cancelamento desfaz `SELECT grava(id)` em silêncio; consumir custa somente tempo |
| Busca na grade | Partes de 8 ms entre as teclas, com 1.000.000 de resultados na lista | Lista sem limite ou leitura em uma tecla | 2 milhões de linhas levam 0,6 s; um texto curto encheria a memória |
| Largura manual | `<`, `>`, `=` e arrasto no cabeçalho | Só o mouse | Toda ação essencial tem um caminho pelo teclado |
| Fechar resultado | `x` ou `Delete` na grade | `Ctrl+W c` | `Ctrl+W c` fecha o console, e essa diferença evita perda por engano |
| Parar uma instrução | Cancelar e fechar como último recurso | Só fechar a conexão | Com a conexão fechada, um `INSERT` terminou nos dois bancos |
| Prazo do cancelamento | 5 segundos | Sem prazo | DataGrip oferece escalonamento parecido |
| Conexão de cancelamento | Temporária e fora do limite do pool | Respeitar o limite literalmente | O protocolo exige outra conexão |

## Arquivos e salvamento

Estas escolhas estão aprovadas em 2026-10-31, com os ajustes registrados no plano.
O código já oferece sessão e cofre. A exceção global autorizada em texto simples ainda não existe.

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Biblioteca JSON | `serde` e `serde_json`, com `deny_unknown_fields` | Leitura manual | Um campo desconhecido invalida o arquivo, como o plano exige; a posição do erro vem do leitor |
| Identidade de um console | UUID `v4` no nome dos arquivos | Contador | Vários computadores criam consoles de um projeto no Git sem colisão |
| Bloqueio de arquivo | `File::try_lock` da biblioteca padrão, em `locks/<id>.lock` | `fs2`, `fd-lock` | O plano pede o bloqueio do sistema, e a biblioteca padrão o oferece sem dependência nova |
| Hash dos arquivos | `DefaultHasher` | `sha2` | O hash só compara conteúdos dentro de um processo, e nunca vai para um arquivo |
| Detecção de mudança | Conferência a cada segundo, por tamanho e data, e depois hash | `notify` | O plano permite observar a pasta ou comparar o hash; a conferência é portátil e não pede uma dependência |
| Criação dos arquivos | Na primeira edição do console | Ao abrir o console | Um console vazio não deixa lixo em um repositório |
| Erro de configuração | Bloqueia as conexões, e `Ctrl+W r` recarrega | Carregar só o escopo válido | O plano proíbe o escopo parcial |
| Mudança externa da configuração | Pergunta, com **Ignore** como foco inicial | Recarga automática | O plano pede a pergunta, e o foco inicial não altera nada |
| Conexão que sai do arquivo | Sai da aplicação só quando está ociosa | Fechar a sessão | O plano proíbe a recarga de alterar conexões ativas |
| Senha no código atual | Sessão ou cofre, com referência `session`, `none` ou `vault`; o segredo não vai para o arquivo | Gravar o segredo no projeto | A exceção global autorizada ainda aguarda implementação e validação do cofre real |
| Ordem do fechamento | Grava o texto, depois fecha | Fechar e gravar | A gravação que falha mantém o console aberto |
| Layout | `workspace.json` do projeto, ou do escopo global sem projeto | Um arquivo por escopo | O layout pertence à sessão, e não à conexão |
| Associação entre escopos | Grava o texto pendente, copia, confere e remove os originais | Mover com `rename` | A pasta de destino pode estar em outro disco |

## Visão de tabela

Estas escolhas estão aprovadas em 2026-10-31.
A nova ordem das abas e o fechamento sem console substituto ainda exigem mudanças no código.

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Abrir uma tabela | `Enter` e duplo clique; `Direita` abre as colunas | `Enter` abre as colunas | `docs/KEYMAP.md` já previa a mudança do `Enter` |
| Ordem das abas | Consoles e tabelas juntos na ordem de abertura | Separar por tipo, como no código atual | Mantém a sequência de abertura do usuário |
| Rótulo da aba | `nome (table)` ou `nome (view)` | Só o nome | O plano pede um rótulo que identifique a view |
| Clique no cabeçalho | Crescente, decrescente e sem ordem | Só crescente | Um clique a mais desfaz a ordem, sem usar o campo |
| Aviso de ordem instável | Uma linha fixa enquanto não existir ordem | Só com mais de uma página | O plano pede o aviso sempre que não houver ordem aplicada |
| Consulta em andamento | Recusar aplicar, limpar, ordenar e carregar mais | Parar e recomeçar | Menos estados; a resposta antiga não encontra a aba em outro estado |
| Fechar o último console | Selecionar uma tabela aberta, sem criar um console | Criar um console vazio, como no código atual | Uma aba de tabela não precisa de um console visível |
| Campos do filtro | Uma linha, com quebras convertidas em espaço | Editor de várias linhas | O filtro é uma condição curta |

## Sessão e conexão

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Estado de sessão | Seletor de banco ou schema por console | Conexão fixa por console | Cinco consoles ocupariam o pool |
| Conectar | No primeiro uso | Ação explícita ou ao iniciar | Pede a senha somente quando necessário |
| Queda de conexão | Substituir ociosas e falhar reservadas | Repetir o comando | Repetir pode duplicar alterações |
| Limpeza falha | Descartar a conexão | Recuperar | Mais simples e seguro |
| Nome da conexão na barra | Encurtar o nome | Encurtar o estado | O usuário precisa ver sempre a transação e a execução |
| Fila do pool | Sem limite | Com limite | Cada console tem uma execução |
| Cancelamento atrasado | Esperar o pedido antes de reutilizar a sessão | Reutilizar ao fim da instrução | Um pedido atrasado poderia atingir a instrução seguinte |
| Fim do pedido no PostgreSQL | Ler até o servidor fechar a conexão do pedido | `cancel_query` do driver | Ele retorna ao escrever, e o `UPDATE` depois de um `SELECT` no teto foi cancelado em um teste com carga |
| `sslmode=require` | Verificar o certificado | Criptografar sem verificar, como no `libpq` | O plano proíbe ignorar a verificação |
| `sslmode=prefer` e `allow` | Recusar o texto | Aceitar | Esses modos podem usar conexão sem TLS em silêncio |
| CA padrão | Sistema e `webpki-roots` | Somente uma das fontes | CAs de empresas ficam no sistema; `mysql_async` já traz a segunda |
| Confiança | Confirmar por destino | Confirmar por projeto | Segue o modelo do SSH e não depende de detectar clones |
| Somente leitura | Campo por conexão, aplicado pelo banco | Confirmar `DELETE` sem `WHERE` | O banco aplica a regra |
| Filtro da tabela | Uma única instrução | Aceitar qualquer texto | Os drivers aceitam várias instruções |
| Limpeza no MySQL | `Conn::reset` e volta ao banco da conexão; sem banco, fechar a sessão | Somente `Conn::reset` | `COM_RESET_CONNECTION` mantém o banco, e um `USE` passaria para o console seguinte |
| Seletor com transação aberta | Recusar a troca | Trocar com `SET` ou `USE` | Uma sessão reservada não recebe o contexto, e uma transação com erro recusa `SET` |
| Desconectar com trabalho aberto | Recusar e nomear o console | Perguntar e parar o console | Menos estados; o usuário resolve a transação com os controles existentes |
| Desconectar com leitura do catálogo | Esperar a sessão por até 5 segundos | Informar o fim logo depois de fechar as ociosas | A mensagem seria falsa, e o formulário ficava preso no modo de renomear |
| Conexão removida | O console fica aberto, sem conexão | Fechar os consoles | O texto não se perde, como nos scripts sem conexão |

## Bibliotecas

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Drivers | `tokio-postgres`, `mysql_async` e `rusqlite` | SQLx | `timeout` falhou, faltam cancelamento e reset, e o pool escondeu conexão recusada |
| Pool | Implementação própria | Pool de biblioteca | Regras próprias de fila, reserva e limpeza |
| Laço principal | Espera de 20 ms na tecla, com canal limitado | Thread só para as teclas | O canal limitado controla a leitura do banco, sem outra thread |
| Tarefa por console | Uma tarefa Tokio guarda a sessão | Mapa de sessões no gerente | A transação fica com seu dono, e o cancelamento fica junto da leitura |
| Tokens por tecla | Reaproveitar os tokens do realce no autocomplete | Analisar o texto duas vezes | Com 50.000 linhas, reduziu de 55 ms para 33 ms |
| Editor | EdTUI com camada convencional | `ratatui-textarea` | Falta realce de sintaxe e modo Vim |
| Análise de SQL | Tokenizador próprio | `sqlparser-rs` ou tree-sitter | Precisa tolerar texto incompleto; protótipo aprovado |
| Realce de sintaxe | Tokenizador próprio com `Highlight` | `syntect` pelo EdTUI | O EdTUI gera RGB e reinicia o estado a cada linha; o plano exige ANSI e `NO_COLOR` |
| Edição no modo convencional | Função própria sobre `state.lines` | Ações do EdTUI | `insert_str` conta bytes, `Paste` trata um LF inicial como linha inteira |
| Desfazer | Pilha do EdTUI, com grupos por palavra | Uma entrada por tecla | A pilha guarda 100 entradas; `DeleteChar(0)` cria o ponto de desfazer |
| Refazer | Contador sobre a pilha do EdTUI | Pilha própria | O EdTUI não limpa `redo` depois de uma nova edição |
| Clique no editor | Cálculo próprio da posição | Mapeamento do EdTUI | O EdTUI conta uma coluna por caractere e para no último caractere |
| Sonda no PostgreSQL | Relógio pelo protocolo simples | `SAVEPOINT` | `SAVEPOINT` gera erros no log |
| Estado no MySQL | `SERVER_STATUS_IN_TRANS` da resposta, e `DO 0` depois de um erro | `SET TRANSACTION`; `performance_schema` | A sonda apagava o `SET TRANSACTION` do usuário e limpava `SHOW WARNINGS`; `performance_schema` exige privilégios |
| Tipos no PostgreSQL | `prepare` antes do protocolo simples | Heurística sobre o texto | `bytea` e números exigem o tipo; custa uma ida ao servidor |
| Provedor do TLS | `ring` | `aws-lc-rs` | Evita um segundo provedor e a compilação de `aws-lc` |

## Produto

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Entrega | Primeira versão completa, com versionamento local | Versões parciais públicas | Decisão do projeto |
| Ordem das etapas | Interface completa antes da visão de tabela | Tabela primeiro, com abertura provisória | A tabela abre pelo navegador e usa as abas; a abertura provisória seria removida depois |
| TSV | Formato do DataGrip, menos as perdas de dados | Cópia exata do DataGrip | A cópia não deve perder dados em silêncio |
| Atalhos no Mac | Sem depender de `Ctrl`+setas, `Home`, `End`, `PageUp` e `PageDown` | Atalhos clássicos de IDE | Parte não funciona, e muitos teclados não têm essas teclas |
| Prefixo | `Ctrl+W` nos dois modos | Fechar aba com `Ctrl+W` | Caminho comum, sem depender do terminal |
| Sair | `Ctrl+W q` | Somente fechar a última aba | A saída com vários consoles precisa de um comando; `q` segue o Vim |
| Adicionar conexão | `a` no navegador | Item na árvore | O navegador vazio já oferece um caminho, e a tecla segue `n`, `e` e `r` |
| Perguntas simultâneas | Fila, com uma pergunta aberta | Substituir a pergunta aberta | Dois consoles podem pedir uma resposta no mesmo instante |
| Nome automático | Menor `console-N` disponível na conexão, entre consoles abertos e salvos | Contador único da aplicação | Os conjuntos de consoles pertencem às conexões |
| Senha no formulário | Campo mascarado; sessão como opção inicial e cofre mediante escolha explícita | Salvar automaticamente | O usuário autoriza o armazenamento do segredo |
| Senha na URI | Mascarar na tela, com o mesmo tamanho | Mostrar o texto digitado | O plano proíbe mostrar senhas; o cursor continua certo |
| Commit por atalho | Sem confirmação | Pedir confirmação | Mesmo comportamento dos botões |
| Metadados | Um `.json` ao lado de cada `.sql` | Índice único ou comentário no script | Reduz conflitos no Git e mantém o script limpo |
| Telemetria | Nenhuma | Opcional | A aplicação só acessa a rede para falar com os bancos |
| Abertura da lista | Ao digitar, além de `Ctrl+Space` | Somente `Ctrl+Space` | A JetBrains é a referência; `Enter` continua a quebrar a linha |
| Caixa das palavras-chave | Segue o texto digitado | Sempre maiúsculas | Respeita o estilo do script |
| Busca | Texto literal, sem distinguir caixa | Expressão regular | Padrão das IDEs; reduz o escopo |
| `COPY` com `STDOUT` | Recusar, como `STDIN` | Tratar como erro do driver | O protocolo não cabe na grade, e o motivo fica claro |
| Posição do erro | Sublinhar o token sem mover o cursor | Mover o cursor | O usuário pode continuar a editar durante a execução |
| Janela de detalhe | Quebrar as linhas sob demanda | `Paragraph` do Ratatui ou texto cortado | Um valor de 40 MB custava 2,3 s por quadro; agora custa 0,4 ms, com o texto completo |
| Substituir tudo | `Ctrl+A` no campo de substituição | `Alt+A` | No macOS, `Option`+letra depende do terminal |
| Arquivo `.sql` externo | Depois da primeira versão | Na primeira versão | Reduz o escopo |
| Licença | `MIT OR Apache-2.0` | GPL ou proprietária | Padrão do ecossistema Rust |
