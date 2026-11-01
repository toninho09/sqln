# SQLN — Registro de decisões

Este documento registra as escolhas e as alternativas rejeitadas.
As regras completas estão no [plano](../PLAN.md).
Não reabrir uma decisão sem um fato novo.

## Versão oficial do EdTUI — 2026-11-01

O usuário removeu o fork `vendor/edtui`, e o projeto volta a usar o `edtui` 0.11.7 publicado.
Esta decisão reabre o item "4. Desfazer" de "Fechamento das decisões" e a linha "Limite do desfazer" de "Credenciais e editor".

| Ponto | Decisão | Rejeitado | Motivo |
|---|---|---|---|
| Limite do desfazer | Sem teto de memória. Cada pilha guarda até 100 entradas | Manter o fork; enviar o limite ao projeto original; publicar o fork com outro nome; histórico próprio | `cargo install` ignora `[patch]`, e o fork bloqueava a publicação. O envio ao projeto original faria o lançamento depender do mantenedor. O histórico próprio não limita a memória no modo Vim |
| Aviso de entrada grande | Removido | Manter o aviso | Sem limite, toda entrada fica no histórico |
| Risco de memória | Débito técnico, descrito em `ARCHITECTURE.md`, seção "Limite conhecido do editor" | Bloquear o lançamento | Escolha do usuário. Scripts comuns ficam muito abaixo do pior caso |

## Linha na vertical, JSON, comentário e total de linhas — 2026-11-01

O usuário aprovou quatro itens da análise do projeto.
O total de linhas não reabre a regra da paginação: uma página continua a pedir uma linha a mais, sem contar o total. A contagem é uma ação separada.

| Item | Ponto | Decisão | Rejeitado | Motivo |
|---|---|---|---|---|
| Linha | Local | Janela sobre a grade, com a tecla `r` | Modo vertical dentro da grade | A janela já tem rolagem, cópia e fechamento. O modo exigiria novo desenho, mouse, busca e seleção |
| Linha | Navegação | `Left`, `Right`, `h` e `l`. Para na última linha carregada, e o cursor da grade acompanha | Ler a próxima página | A janela não faz consulta |
| Linha | Valor longo | Até 4.096 colunas, com `…` | Valor inteiro | Um valor enorme atrasaria a tecla, e `Enter` mostra o valor inteiro |
| JSON | Forma | Automática, com `f` para trocar. **Format JSON** em Settings define a forma inicial, ligada por padrão | Só automática | Pedido do usuário |
| JSON | Cópia | `Ctrl+C` copia a forma visível | Sempre o original | O usuário escolhe a forma com `f`, e a grade copia o original |
| JSON | Motor | Reescreve só os espaços e valida com `IgnoredAny` | `serde_json::Value` | `Value` reordena as chaves e perde dígitos, e a regra proíbe alterar valores |
| JSON | Limites | Entrada até 1 MiB, saída até 8 MiB. Acima disso, o valor fica como texto | Sem limite | Cada linha de um JSON profundo repete o recuo, e 200 KB de `[` gerariam dezenas de GB |
| Comentário | Teclas | `Ctrl+W /`, `Ctrl+/` e `Ctrl+7` | Só `Ctrl+W /` | Pedido do usuário. É uma exceção ao critério 1 de `KEYMAP.md`, e `Ctrl+W /` atende ao critério 2. Sem aprimoramento, o terminal envia `Ctrl+/` como `Ctrl+7` |
| Comentário | Regra | Sem seleção, a linha do cursor. Linhas em branco ficam iguais. Todas comentadas perdem o comentário; nos outros casos, o comentário entra no menor recuo | Comentar cada linha no próprio recuo | É o costume das IDEs, e o texto mantém a forma |
| Total | Forma | Tecla `c`, com `COUNT(*)` exato e o filtro aplicado | Estimativa automática; `COUNT(*)` automático | `COUNT(*)` em uma tabela grande segura uma conexão, e a estimativa erra com filtro e não existe no SQLite |
| Total | Caminho | O mesmo de uma página | Método novo no gerente | Prazo, Stop, pool, senha e orçamento valem sem código novo |
| Total | Validade | **Apply** e **Clear** limpam o total, e a ordenação mantém | Limpar sempre | A ordem não muda a contagem |
| Total | Disponível | Só com mais linhas para carregar, em uma aba de tabela ou view | Toda aba | Sem mais linhas, a grade já tem o total. Chave Redis e estrutura não têm SQL de contagem |

### Limitações conhecidas

- O comentário lê linhas, não tokens. Uma linha dentro de uma string ou de um comentário de bloco recebe o prefixo.
- `--x` no MySQL não é comentário, mas a remoção o trata como comentário.
- O total fica velho quando outra sessão muda a tabela, até o próximo **Apply**.
- Uma coluna JSON sem formato de objeto ou array, como um número ou uma string, não troca de forma.

## Alvo, proteção da execução, formatador e histórico — 2026-11-01

O usuário aprovou cinco itens da análise do projeto.
O histórico reabre a seção 17 do plano, que o deixava fora da primeira versão.

| Item | Ponto | Decisão | Rejeitado | Motivo |
|---|---|---|---|---|
| Alvo | Conexão | `sqln arquivo.db` e `sqln postgres://...` duram só a sessão | Salvar em `config.json` | Uma URI nova criaria um cadastro a cada uso |
| Alvo | Texto do console | Fica em memória, e a saída avisa antes de perder o texto | Salvar em um script | O script precisa de uma conexão salva |
| Alvo | URI sem senha | PostgreSQL e MySQL: o terminal pede a senha oculta antes da interface. `Enter` vazio segue sem senha. Redis não pergunta | Falhar com erro | A senha fora da URI não vai para o histórico do shell |
| Alvo | Senha recusada | O erro aparece em Mensagens, e o usuário repete o comando | Pedir na interface | O pedido da interface exige um `ConnectionKey` |
| Alvo | Quantidade | Um por comando | Vários | YAGNI |
| Alvo | Início | Sem **Create project**. As abas salvas não abrem e ficam no arquivo | Manter os dois | O usuário já disse o que quer abrir |
| Alvo | Arquivo inexistente | O console espera o diálogo de criação | Ligar à primeira conexão salva | O console mudaria de banco sem aviso |
| `WHERE` | Instruções | `UPDATE` e `DELETE` sem `WHERE` no nível principal | Incluir `TRUNCATE` e `DROP` | Esses dois não têm `WHERE`; a marca de produção cobre os dois |
| `WHERE` | Conexões | Todas, menos as somente leitura, sem opção para desligar | Só produção | Um `DELETE` sem `WHERE` custa caro em qualquer banco |
| `WHERE` | Modo manual | Também avisa | Pular | Regra única |
| Produção | Efeito | Selo `PROD` e confirmação antes de uma execução que não seja leitura simples | Só o selo | O selo sozinho não impede um engano |
| Produção | Sem pergunta | Leitura simples, `BEGIN`, `START`, `COMMIT`, `END`, `ROLLBACK`, `SAVEPOINT`, `RELEASE` e `USE` | Perguntar em tudo | `SET` pergunta, pois `SET GLOBAL` muda o servidor |
| Produção | Selo | Na barra de status, com texto e sem sair por falta de largura | Abas e navegador | O texto vale com `NO_COLOR` |
| Produção | Conexão ativa | A marca muda sem reconectar | Exigir desconexão | A marca não muda a sessão |
| Diálogo | Forma | Um por execução, com foco em **Cancel**, contagem e primeira instrução de risco | Um por instrução; digitar o nome | Um script longo não vira uma fila de perguntas |
| Formatador | Motor | Próprio, sobre o lexer do projeto | `sqlformat` | O crate não tem dialeto MySQL, e o lexer já conhece os dialetos do projeto |
| Formatador | Segurança | Fora de uma quebra de linha, o gap entre tokens só encolhe para um espaço. A conferência compara todos os tokens e recusa uma diferença | Refazer os espaços | O lexer guarda `>=` como dois tokens |
| Formatador | Strings | Duas strings em duas linhas ficam em duas linhas | Juntar na mesma linha | Em SQL, juntar as duas na mesma linha é erro |
| Formatador | Alcance | A seleção; sem seleção, a instrução sob o cursor | O script inteiro | É a regra do Run |
| Formatador | Instruções | `SELECT`, `WITH`, `VALUES`, `INSERT`, `UPDATE` e `DELETE` | Todas | Rotinas têm corpo próprio, e o risco não compensa |
| Formatador | Caixa | Mantém o texto digitado | Maiúsculas | Segue a decisão do autocomplete |
| Formatador | Lista | O primeiro item fica na linha da cláusula | Quebrar depois da cláusula | Não exige olhar à frente, e uma lista de um item fica em uma linha |
| Histórico | Arquivo | `history.jsonl` na pasta global, permissão `0600`, ligado por padrão | Por projeto; só em memória | Projetos antigos não têm essa linha no `.gitignore` |
| Histórico | Conteúdo | Cada instrução de console que chegou ao banco, com hora, conexão e sucesso ou falha | Tempo e linhas | YAGNI |
| Histórico | Fora | Tabelas, catálogo, exportação e conexões sem cadastro | Registrar tudo | Só o texto do usuário interessa |
| Histórico | Segredos | Não grava `PASSWORD`, `PASSWD`, `IDENTIFIED`, `REQUIREPASS` e `MASTERAUTH`, nem Redis `AUTH`, `ACL` e `HELLO` | Gravar tudo | Segue a regra do cliente `mysql` |
| Histórico | Limites | Arquivo até 8 MiB; na abertura, um arquivo maior guarda os 4 MiB mais novos. Uma instrução acima de 64 KiB e uma repetição seguida não entram | Sem limite | O arquivo cresce sem limite e a janela lê tudo |
| Histórico | Escrita | Uma escrita por quadro, com o arquivo aberto de novo | Arquivo aberto o tempo todo | `trim` troca o arquivo, e outra instância continuaria a gravar no arquivo antigo |
| Histórico | Janela | `Ctrl+W H` e paleta. Lista a conexão do console, com filtro por palavras | Todas as conexões | O SQL de outro banco raramente serve |
| Histórico | `Enter` | Insere no cursor e não executa | Executar | O usuário revisa antes |

### Limitações conhecidas

- Uma senha na URI fica no histórico do shell e em `ps`. Omita a senha, e o terminal pede.
- Uma conexão de linha de comando não reage a uma senha recusada. Repita o comando.
- `DELETE t FROM t JOIN u ON ...` no MySQL pergunta, mesmo com o `JOIN` limitando as linhas.
- Um `WHERE 1 = 1` satisfaz a proteção.
- `SELECT minha_funcao()` pergunta em produção, pois o texto não prova que a função só lê.
- Redis tem só o selo. A confirmação de produção não existe nessa fonte.
- O filtro de segredos lê palavras. Um segredo dentro de um literal, como `WHERE token = 'abc'`, entra no arquivo.
- Uma coluna chamada `password` deixa a consulta fora do histórico.
- Uma instrução gravada por outra instância durante `trim` pode se perder.
- Uma palavra sem reserva no MySQL, como uma coluna `offset`, pode começar uma linha. O SQL continua válido.
- `CREATE`, `ALTER` e rotinas não têm formatação.

## Linha de dicas — 2026-10-31

O usuário pediu que o rodapé sempre mostre os comandos mais relevantes para o momento.
Uma auditoria achou comandos úteis fora da linha e comandos sem efeito dentro dela.

| Ponto | Decisão | Rejeitado | Motivo |
|---|---|---|---|
| Regra | Cada estado lista os comandos que agem nele, em ordem de relevância | Lista fixa por painel | Uma tecla sem efeito gasta espaço, e uma tecla útil fica escondida |
| Espaço | Um comando que não cabe sai inteiro, e um comando curto depois dele ainda entra | Cortar a linha na borda; exigir que cada estado caiba em 80 colunas | A ordem decide o que uma tela pequena mostra, e nenhuma tecla fica cortada. Isso substitui a regra de fechar a linha do explorador em 80 colunas |
| Parar | Primeiro comando em toda a aba ocupada | Só no editor | Uma leitura longa ocupa uma sessão, e o foco pode estar na grade |
| Transação | `commit` e `rollback` vêm primeiro no editor e nos resultados | Só nos botões | A transação segura uma sessão até o usuário decidir |
| Navegador | Os comandos mudam com a linha do cursor. Os comandos da conexão ficam na linha da conexão | Mesma lista em toda linha | Em uma tabela, `Del` removeria a conexão inteira, e essa dica não ajuda ali. As teclas continuam a agir, e `F1` lista todas |
| Sem conexão | O editor troca `run` e `script` por `connection` e `navigator` | Manter `run` | Sem conexão, `F5` só mostra um aviso |
| Ordem das janelas | A escolha das dicas segue a ordem de `on_key` | Manter a ordem própria | A linha precisa nomear as teclas da janela que recebe as teclas |

## Aba de estrutura — 2026-10-31

O usuário pediu uma forma de ver o schema de uma tabela e de um banco, e reabriu a regra que excluía a aba de estrutura da primeira versão.
O navegador mostrava só os nomes das colunas.

| Ponto | Decisão aprovada | Rejeitado | Motivo |
|---|---|---|---|
| 1. Local | Aba própria na faixa principal, com grade somente leitura | Alternar na aba da tabela; console gerado; tipos no navegador | Reaproveita busca, cópia, exportação, parada e pool. Não mistura filtro e paginação com a estrutura |
| 2. Conteúdo da tabela | Colunas (tipo, nulo, padrão, chave primária), índices e chaves estrangeiras | Só colunas; incluir o DDL | O PostgreSQL não tem `SHOW CREATE TABLE`, e um texto montado pela ferramenta poderia divergir do real |
| 3. Schema ou banco | Todas as colunas, uma linha por coluna | Só as relações; nada | A busca da grade acha uma coluna em qualquer tabela |
| 4. Redis e Valkey | Recusar com aviso na barra de status | Mostrar tipo, TTL e memória da chave | Uma chave não tem colunas |
| 5. Teclas | `i` abre a estrutura. `[` e `]` trocam a seção | Tecla própria para a seção | `[` e `]` já trocam de resultado no console |
| 6. Reinício | A aba de estrutura não volta na próxima sessão | Salvar em `OpenTabs` | O custo de abrir é uma tecla |
| 7. Atualização | **Reload** e a troca de seção releem a grade. DDL não atualiza a aba sozinho | Guardar as três seções | A leitura é curta, e sem guardar nada a grade nunca fica velha sem aviso |
| 8. Muitas linhas | A grade usa a paginação e o orçamento da tabela | Ler tudo de uma vez | Um banco grande pode ter muitas colunas |
| 9. Exportação | Seleção, linhas carregadas e **All rows** funcionam, sem tabela de destino | Bloquear a exportação | A grade não é uma tabela, então o SQL INSERT pede o nome, como no console |
| 10. Linha de conexão | `i` mostra todos os schemas que o navegador mostra | Só o schema atual | Atende ao pedido de ver o banco inteiro |
| 11. Linha de coluna | `i` abre a estrutura da tabela dessa coluna | Não fazer nada | A coluna pertence a uma tabela |

Decisões de implementação:

| Ponto | Decisão | Rejeitado | Motivo |
|---|---|---|---|
| Aba | `TableTab` com `structure` e `filters = false` | Tipo novo de aba | A aba de tabela já tem prazo, parada, fila do pool, orçamento, senha e páginas |
| Leitura | SQL por `Session::run`, como uma página | Novo método em `Session` | Nenhum conector muda, e a leitura aceita parada e prazo |
| Nome em literal | PostgreSQL usa `E'...'` | `quote_literal` nos três bancos | Com `standard_conforming_strings = off`, um nome hostil do catálogo poderia fechar um literal comum |
| Linha de dicas | Pedido do usuário: `i struct` aparece no navegador e na grade de uma tabela | Deixar `i` só no `F1` e na paleta | O usuário não achou a tecla. Veja a seção **Linha de dicas** |

### Limitações conhecidas da estrutura

- Um índice de expressão mostra `expression` no MySQL e no SQLite. O PostgreSQL mostra a expressão.
- Uma chave estrangeira do SQLite não tem nome, então a coluna `name` mostra o número da chave.
- Com `NO_BACKSLASH_ESCAPES` no MySQL, um nome com barra invertida não encontra a tabela e a grade fica vazia. O nome continua dentro do literal.

## Explorador de arquivos — 2026-10-31

**Definido:** um botão `...` ao lado dos campos de caminho abre um explorador, e o usuário aprova as 10 decisões abaixo.
O campo continua editável. O explorador só ajuda a preencher.

| Ponto | Decisão aprovada | Rejeitado | Motivo |
|---|---|---|---|
| 1. Tipo | Janela própria no terminal, sem dependência nova | Diálogo nativo com `rfd`; só completar o caminho | O diálogo nativo falha em SSH e num Linux sem portal, e tira o foco do terminal. Completar não mostra as pastas |
| 2. Abrir pelo teclado | `Ctrl+O` no campo. A barra de dicas mostra a tecla, o `...` serve ao mouse, e a ordem do `Tab` não muda | O `...` como parada do `Tab` | A parada acrescenta até 3 passos no formulário de rede |
| 3. Campos | File do Export, File do SQLite, CA, certificado do cliente e chave do cliente | Só File do Export e do SQLite | O custo por campo é pequeno, e a tela fica coerente |
| 4. Arquivo novo | Linha **Use this folder** no topo, só no Export e no SQLite. Ela troca a pasta e mantém o nome do campo | Campo de nome e botões na janela; só arquivos que existem | Um modo só. Com o campo vazio, ele recebe a pasta, e o usuário digita o nome |
| 5. Teclas | Como na paleta: letras filtram e setas movem. `←` e `Backspace` com filtro vazio sobem uma pasta. `→` e `Enter` entram. Vale nos dois mapas de teclas | `j k h l` e `/` no modo vim | Repete um padrão do app e evita dois modos de tecla |
| 6. Arquivos mostrados | Todos | Filtro por extensão | Um banco SQLite aceita qualquer extensão, e o filtro digitado resolve o resto |
| 7. Ocultos | Escondidos. Um filtro que começa com `.` mostra todos | Sempre visíveis; uma tecla alterna | Não usa tecla nova. `Ctrl+H` não serve, pois muitos terminais o enviam como `Backspace` |
| 8. Mouse | Clique move o cursor, duplo clique abre, e a roda rola | Clique único abre | Com clique único, o duplo clique por hábito entra na pasta e ativa outra linha |
| 9. Pasta inicial | Pasta do caminho do campo. Sem ela, a pasta existente mais próxima. Campo vazio: pasta atual | Sempre a pasta atual; lembrar a última pasta | O Export já começa na pasta salva, e o campo já traz essa pasta |
| 10. Home | `~` com filtro vazio vai para a home | Sem atalho | Uma tecla, e o campo File do Export já aceita `~` |

Estas escolhas seguem as regras acima nos pontos que elas deixaram abertos.

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Caminho no campo | Sempre absoluto | `~/...` | O campo do SQLite não expande `~` |
| Extensão do Export | **Use this folder** mantém a extensão ligada ao formato. Escolher um arquivo desliga essa ligação | Sempre desligar | A troca de pasta não muda o nome |
| Replace | A janela continua no botão **Export** | Perguntar no explorador | O campo ainda pode mudar antes do Export |
| Filtro com texto | Esconde `../` e **Use this folder** | Manter as linhas fixas | `Enter` age no primeiro resultado |
| Caminho que o campo alteraria | Uma nota recusa o caminho com caractere de controle, com espaço nas pontas ou fora de UTF-8 | Preencher assim mesmo | O campo guardaria outro caminho, sem aviso |
| Link para pasta | Aparece como pasta, e `../` volta à pasta de origem | Mostrar o destino real | O usuário volta para onde estava |
| Fora do escopo | Criar pasta, renomear, apagar, tamanho, data e favoritos | | O Export já cria as pastas que faltam |
| Tamanho dos modais | Pedido do usuário: o Export, o formulário de conexão e o explorador crescem até 140 colunas e deixam 8 livres. O explorador fica 8 colunas mais estreito que o modal, e sua lista vai de 14 a 35 linhas | Tamanho fixo; crescer sem limite | Um caminho longo aparece inteiro, e um limite mantém o rótulo perto do valor |

O usuário também aprovou o crescimento dos outros modais. Cada modal cresce só com espaço livre, deixa 8 colunas livres, e nunca fica menor que antes.

| Modal | Decisão aprovada | Rejeitado | Motivo |
|---|---|---|---|
| Settings | Até 140 colunas. A altura não muda | Até 100 colunas | Settings e Export têm o mesmo layout |
| Paleta de comandos | Até 100 colunas e 30 linhas. Continua no topo | Só mais linhas | Mostra mais comandos sem rolar |
| Listas de schema, consoles, scripts e conexões | Até 100 colunas e 30 linhas. A altura segue a quantidade de itens | Tamanho fixo | Um nome longo de script era cortado |
| Conflitos de scripts | Até 100 colunas e 30 linhas. O nome recebe as colunas novas | Tamanho fixo | O nome identifica o script |
| Renomear e senha | Até 100 colunas. A altura não muda | Tamanho fixo | O destino da conexão e o nome têm mais espaço |
| Perguntas | Até 100 colunas | Limite de 72 colunas | Um caminho longo quebra em menos linhas |
| Detalhe da célula e ajuda | Até 140 colunas | Limite de 90 colunas | Um valor longo mostra mais texto em cada linha |

O menu do `Ctrl+W` não cresce, pois ele não é modal e fica preso ao canto da tela.

### Limitações conhecidas do explorador

- A leitura da pasta fica na thread da interface, como `is_dir` e `exists` em `destination_of`. Um disco de rede lento pode travar a tela durante a leitura.

## Exportação para arquivo — 2026-10-31

**Definido:** o usuário pede a exportação de resultados por um modal e aprova as 25 decisões abaixo.
Este pedido reabre as regras "Exportação para arquivo" e "Extratores além do TSV padrão" da lista de itens fora da primeira versão. O editor de extratores continua fora.

| Ponto | Decisão aprovada | Rejeitado | Motivo |
|---|---|---|---|
| 1. Alcance | O modal oferece Selection, Loaded rows e All rows. All rows relê o banco, sem a grade e sem o teto de 256 MiB | Só linhas carregadas; só todas as linhas | A grade guarda 1.000 linhas por padrão, e um arquivo incompleto sem pedido é o pior erro de uma exportação |
| 2. Reler no console | Só com `is_plain_read`. Outro comando fica só com Loaded rows, e o modal explica o motivo | Qualquer comando com aviso; nunca no console | Reler repete o comando, e um comando que grava repetiria a gravação |
| 3. Formatos | CSV, TSV, JSON, JSON Lines e SQL INSERT | XLSX | XLSX traz uma dependência nova |
| 4. Destino | Só arquivo; `Ctrl+C` não muda | Arquivo e área de transferência | Um fluxo só |
| 5. Execução | Na tarefa da aba, na mesma sessão. O modal fecha, a aba fica ocupada, a linha de status mostra as linhas gravadas, Stop cancela, e a grade mantém o resultado | Segundo plano com conexão própria | Uma transação aberta vê os mesmos dados, e a aba não ocupa outra conexão |
| 6. Tempo | All rows ignora `sql_timeout`; Stop é a saída | Mesmo `sql_timeout` | O padrão de 30 segundos encerraria uma exportação grande |
| 7. Parcial | Grava em `<nome>.tmp`. Em Stop ou erro, uma janela pergunta: manter (renomeia para o destino) ou apagar | Sempre apagar; sempre manter | É uma escolha do usuário em cada caso |
| 8. Arquivo existente | Janela com Replace e Cancel; Cancel volta ao modal | Recusar; criar outro nome | Nenhum arquivo muda sem confirmação |
| 9. Opções de CSV e TSV | Cabeçalho (ligado), delimitador só no CSV (`,`, `;`, `\|`), texto do NULL (vazio ou `NULL`) e BOM UTF-8 (desligado) | Menos opções | O Excel em português espera `;`, e sem BOM ele quebra os acentos |
| 10. Binário | CSV e TSV seguem a regra da grade. JSON usa Base64 em todo `Value::Binary`. SQL usa o literal do dialeto | Regra da grade também no JSON | Base64 é o costume em JSON |
| 11. Nomes repetidos no JSON | Sufixo numérico: `id` e `id_2` | Recusar o JSON; repetir a chave | Nenhum valor se perde, e o JSON continua válido |
| 12. Agrupar INSERT | O modal mostra a escolha: um por linha (padrão) ou agrupado | Sem escolha | É uma escolha do usuário em cada caso |
| 13. Lote do INSERT | Campo só com agrupado, padrão 100 | Fixo em 100 | O MySQL recusa um comando acima de `max_allowed_packet` |
| 14. Números no JSON | Número com o texto do banco. `NaN` e `Infinity` viram string | Sempre string; opção no modal | O arquivo guarda todos os dígitos |
| 15. Scripts | All rows só com um comando na execução ou com transação aberta | Permitir com aviso; reler o script inteiro | A sessão é limpa entre execuções, então reler só o `SELECT` depois de um `SET` pode ler outra tabela sem erro |
| 16. Redis e Valkey | Só Selection e Loaded rows, sem SQL INSERT | Tudo nas abas de chave | Não existe `is_plain_read` para Redis, e a aba de chave lê por páginas |
| 17. Abrir o modal | Tecla `e` na grade e ação na paleta | Também no navegador; só paleta | A tecla fica perto da grade, e `e` está livre nos dois mapas de teclas |
| 18. Caminho inicial | Pasta salva ou pasta atual, com `<origem>-AAAA-MM-DD-HHMM.<ext>`. A extensão segue o formato até o usuário editar o caminho | Sem data; pasta Downloads | A janela de arquivo existente quase nunca aparece |
| 19. Lembrar escolhas | Formato, opções e pasta persistem | Só na sessão; não lembrar | Quem usa `;` não escolhe de novo |
| 20. Vários resultados | Só o resultado ativo; um modal gera um arquivo | Um arquivo por aba de resultado | Multiplicaria as janelas de arquivo existente e de parcial |
| 21. Navegador | Fora do escopo | Tecla na tabela do navegador | O usuário retirou o pedido |
| 22. Onde salvar | `preferences.export` no `config.json` global, ao confirmar o Export | Escopo ativo; `workspace.json` | O `config.json` de um projeto é versionado, e a pasta seria um caminho de uma máquina só |
| 23. Layout | Barra lateral com os formatos, igual ao diálogo Settings | Formulário em uma coluna | Mostra todos os formatos de uma vez, e o mouse escolhe com um clique |
| 24. Alcance inicial | Selection, se existir. Senão All rows, se disponível. Senão Loaded rows | Selection ou Loaded rows | Evita um arquivo incompleto por engano |
| 25. Exportações simultâneas | Uma por aba. O modal recusa um destino em uso | Uma por vez | Cada aba tem a sua tarefa, e dois destinos iguais usariam o mesmo temporário |

Estas escolhas seguem as regras acima nos pontos que elas deixaram abertos.

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Filtro da tabela | All rows usa o filtro e a ordem aplicados | O texto digitado | A grade mostra o que foi aplicado |
| `Enter` no modal | Só o botão **Export** exporta. Em outro campo, `Enter` vai ao próximo, como no diálogo Settings | `Enter` exporta de qualquer campo | Pedido do usuário: evita uma exportação sem querer |
| All rows oculto | Quando a grade já tem tudo, só Loaded rows aparece | Reler mesmo assim | Evita uma consulta sem ganho |
| Quando abrir | Só com a aba livre e um resultado ativo | Abrir na aba **Messages** | Sem resultado não há o que gravar |
| Zero linhas | Gera um arquivo: só cabeçalho, `[]` ou vazio | Recusar | O resultado vazio também é uma resposta |
| JSON | Um objeto por linha dentro do array, sem indentar campos; a saída é UTF-8 | Indentação completa | O arquivo fica pequeno e legível |
| Campo File | Aceita `~`, cria as pastas que faltam e recusa uma pasta como destino | Exigir uma pasta existente | É o que um usuário de terminal espera |
| Nome de origem | Troca `/`, `\`, `:` e caracteres de controle por `-`; a hora é local | Hora UTC | O nome precisa ser válido e legível |
| Sem linhas gravadas | Stop ou erro antes da primeira linha apaga o temporário, sem janela | Mostrar a janela | Não há nada para manter |
| **Keep** | Fecha o formato, então o JSON parcial continua válido; a janela avisa quando substitui um arquivo antigo | Deixar o arquivo aberto | O parcial continua legível |
| Fechar durante a exportação | Usa a confirmação atual de execução, e o temporário é apagado | Janela de parcial | A aba some, e a janela não teria contexto |
| Permissão | Padrão do sistema | `0600` | O arquivo é do usuário, como o de qualquer programa |
| Falha ao salvar as escolhas | Mostra uma nota, e a exportação continua | Cancelar | A exportação é o pedido principal |
| `preferences.export` no projeto | Erro de configuração | Ignorar | O campo existe só no escopo global |
| Progresso | A linha de status lê um contador da exportação | Um evento a cada lote | O canal tem 256 posições e a tela lê um número limitado de eventos por quadro, então os eventos poderiam atrasar a gravação |
| Hora local | `chrono` com a feature `clock` | `time`; `libc` | `time` recusa o fuso local em um processo com várias threads, e o projeto não tem `unsafe` |

### Limitações conhecidas da exportação

- Uma coluna JSON do banco vira string, pois `Value` não guarda o tipo da coluna.
- `is_plain_read` recusa uma função desconhecida. `SELECT minha_funcao(x)` fica com Loaded rows.
- All rows lê os dados de agora, que podem diferir da grade.
- O CSV não neutraliza fórmulas como `=SOMA(...)`, pois o projeto não altera valores.
- A troca final do temporário não confere o destino de novo.

## Redis e Valkey — 2026-10-31

**Definido:** o usuário pede suporte a Redis e Valkey e aprova as 11 recomendações abaixo.
Este pedido reabre a regra "MongoDB, Redis e Kafka ficam fora da primeira versão" somente para Redis e Valkey.

| Ponto | Decisão aprovada | Rejeitado | Motivo |
|---|---|---|---|
| 1. Entrada no console | Um comando por linha, como no `redis-cli`; `#` no início da linha é comentário | `;` termina o comando | O texto do `redis-cli` funciona sem mudança |
| 2. Explorador | Bancos e até 1.000 chaves por banco, com `SCAN`; `Enter` abre o valor em uma aba | Só bancos com lista paginada; só bancos | Permite navegar sem escrever comandos |
| 3. Somente leitura | Consultar `COMMAND INFO` e recusar comandos sem a marca `readonly`; sem resposta do servidor, recusar | Lista fixa; retirar a opção | Cobre Valkey e módulos, e o servidor não tem sessão somente leitura |
| 4. Transações | Sem modo manual; `MULTI` e `EXEC` funcionam em um script | Ligar o modo manual a `MULTI` e `EXEC` | Dentro de `MULTI`, uma leitura responde `QUEUED`, sem dados |
| 5. Comandos contínuos | Recusar `SUBSCRIBE`, `PSUBSCRIBE`, `SSUBSCRIBE` e `MONITOR` | Fluxo contínuo na grade | O console não tem fluxo contínuo |
| 6. Comandos destrutivos | Sem confirmação | Pedir confirmação | `DROP` também não pede confirmação |
| 7. TLS de `redis://` | Regra do SQLN: local sem TLS, remoto verifica; `?sslmode=disable` desliga; `rediss://` sempre usa TLS | `redis://` nunca usa TLS | A senha só viaja sem TLS por escolha explícita |
| 8. Topologia | Somente servidor único | Cluster; Cluster e Sentinel | Cluster e Sentinel mudam o pool |
| 9. Resultado na grade | Valor em uma célula, lista em linhas, mapa nas colunas `field` e `value`, valor aninhado como texto | Tudo como texto | Aproveita a grade; exige RESP3 |
| 10. Autocomplete | Nomes de comandos, com lista fixa | Também nomes de chaves | A lista de chaves pode ser parcial |
| 11. Tipo de conexão | Um tipo "Redis/Valkey", com `redis://`, `rediss://`, `valkey://` e `valkeys://` | Dois tipos | O protocolo é o mesmo |

Estas escolhas seguem as regras acima nos pontos que elas deixaram abertos.

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Driver | `redis` 1.8, com `MultiplexedConnection` em um stream próprio | TLS do driver | O TLS do driver não usa as raízes de `connector::tls`; o stream próprio mantém uma regra de TLS para todas as fontes |
| Banco no formulário | Número, validado no formulário, na URI e no `config.json` | Texto livre | Redis não tem nomes de banco |
| Usuário | Opcional somente para Redis | Exigir `default` | Um servidor sem ACL não tem usuário |
| Comandos de identidade | Recusar `AUTH`, `HELLO` e `RESET` | Permitir | A sessão volta ao pool, e outro console receberia o novo login |
| Replicação | Recusar `SYNC` e `PSYNC` com os comandos contínuos | Permitir | Esses comandos também não terminam |
| Testes | `tests/redis.rs`, separado de `tests/servers.rs` | Somar ao `tests/servers.rs` | Esse arquivo repete SQL em cada servidor |

### Limitações conhecidas de Redis e Valkey

| Assunto | Limitação |
|---|---|
| Somente leitura | A proteção fica só no cliente. `PING`, `INFO`, `ECHO`, `TIME`, `SELECT`, `CONFIG`, `CLIENT`, `OBJECT`, `MEMORY` e `XINFO` ficam recusados, pois o servidor não marca esses nomes como `readonly`. O seletor de banco continua a funcionar |
| Stop | Só para um comando que espera, como `BLPOP`. Em outro comando, o SQLN fecha a conexão, e o servidor termina o comando |
| Comando lento | Um `KEYS *` longo prende o servidor, então o pedido de parada pode esperar 5 segundos sem resposta |
| Teto e orçamento | O driver lê a resposta inteira antes do teto de linhas e do orçamento da grade. Um `LRANGE k 0 -1` enorme usa memória antes do corte |
| Explorador | Mostra até 1.000 chaves na ordem de chegada do `SCAN`, ordenadas por nome. Bancos vazios não aparecem, exceto o banco atual. Uma chave que não é UTF-8 não aparece e marca a lista como parcial. Um comando não atualiza a árvore; use **Refresh** |
| Páginas de valor | `HSCAN`, `SSCAN` e `XRANGE` não têm offset, então cada página repete a leitura anterior, com o custo de `OFFSET` em SQL. Um hash ou set que muda pode repetir ou pular itens |
| Tipos de módulo | Um tipo de módulo, como JSON, não abre em aba. Use o console |
| `MULTI` | Vale somente dentro da mesma execução. Executar `MULTI` sozinho descarta a fila no fim da execução |
| `CLIENT REPLY OFF` | O SQLN não recusa esse comando. A sessão fica sem resposta até o prazo ou até o Stop |
| Topologia | Sem Cluster e sem Sentinel. Uma resposta `MOVED` aparece como erro |
| Versão | RESP3 exige Redis 6 ou mais novo. Um servidor mais antigo falha ao conectar |
| Autocomplete | A lista de comandos é fixa, sem comandos de módulo e sem chaves |
| Linha de comando | Um valor não ocupa duas linhas. Use `\n` entre aspas duplas |

### Pendências de Redis e Valkey

- Validar em um terminal real: cadastro, explorador, aba de valor e cofre.
- Testar um socket Unix com servidor real. O Docker no macOS não compartilha esse socket, e o código só tem teste de compilação.
- Testar o Stop de um comando que não espera. Um script Lua longo prenderia os outros testes no mesmo servidor.
- Testar um servidor com ACL restrita, sem `CLIENT ID` ou sem `COMMAND INFO`.
- Testar serviços gerenciados, que podem renomear comandos.

## Interface no estilo nvim — 2026-10-31

**Definido:** o usuário pede uma interface mais bonita, próxima do nvim e de uma IDE.
O usuário aprova estes quatro pontos antes da implementação.
O código já implementa os quatro pontos.

| Ponto | Decisão aprovada | Rejeitado | Motivo |
|---|---|---|---|
| Cores | Tema `astrodark` em RGB com truecolor; tema ANSI nos demais casos; `NO_COLOR` sem cor | Somente ANSI | Fundos e tons próprios separam os painéis; a paleta vem do tema do nvim do usuário. Esta decisão substitui "usar as cores ANSI do terminal" |
| Layout | Sem caixas: abas em toda a largura, explorador, título de painel, barra de status e linha de dicas | Caixa com borda em cada painel | Cada caixa gasta duas linhas e duas colunas, e 80 x 24 é o mínimo |
| Ícones | Unicode por padrão; `icons: "nerd"` em `config.json` | Somente Nerd Font | A fonte Monaco do usuário não tem esses símbolos |
| Ícone da conexão | Com `icons: "nerd"`, o logo do banco fica antes do nome, salvo com um ícone escolhido na conexão. No conjunto Unicode, `⛁` fica antes do nome e `pg`, `my`, `sq` ou `rd` depois. `NO_COLOR` não tem ícone | Emoji, que ocupa 2 colunas e não é logo; imagem no terminal, ilegível em 1 ou 2 células | O Unicode não tem esses logos. No iTerm, o Nerd Font Bundle mostra os logos e mantém a Monaco no texto |
| Temas | Esquemas de cores como dados em `ui/schemes`, escolhidos em `Settings > Appearance`, com prévia antes de salvar. Detalhes em "Temas — 2026-10-31" | Uma função de estilos para cada tema | Cada tema novo vira uma tabela de cores, e os testes conferem o contraste |
| Recursos | Menu do `Ctrl+W`, paleta de comandos, tela inicial, barras de rolagem, linha atual e número de linha na grade | Somente mudanças visuais | O usuário também pede uma interface mais intuitiva |

Estas escolhas seguem as regras acima nos pontos que elas deixaram abertos.

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Botões de página na tabela | **Previous** e **Next** movem o cursor nas linhas acumuladas; **Next** carrega a página que falta | Trocar as linhas a cada página, com nova consulta para voltar | Mantém a regra de manter as linhas anteriores, o orçamento e a consulta por página; voltar não repete a consulta |
| Botões de página no console | Não oferecer | Nova consulta por página | Vale a decisão de mais linhas no console: repetir a consulta altera o SQL e pode mostrar outro resultado |
| Tecla que aceita sugestão | Opção `suggestionKeys`: `tab` (padrão), `enter` ou `both`, no editor e no filtro da tabela | Somente `Tab`, como no plano original | Reaberta pelo usuário em 2026-10-31; o padrão mantém a regra antiga, e `Enter` só aceita por escolha explícita |
| Precisão do tempo | Três casas decimais durante a execução e no último tempo, com atualização a cada 100 ms | Segundos inteiros; 250 ms | Pedido do usuário; a 250 ms os últimos dígitos ficam quase parados |
| Estado da execução nas abas | Selo com fundo colorido no início da aba, com o tempo durante a execução | Símbolo no fim da aba, ao lado do nome | No fim, `✗` ficaria ao lado do `×` de fechar; um bloco de cor é difícil de perder |
| Fim do selo de resultado | O selo sai quando o usuário deixa a aba, ou com a próxima execução | Manter até a próxima execução; sair ao entrar na aba | Uma aba com selo significa um resultado que o usuário ainda não viu |
| Início do tempo | Quando a execução começa, sem a espera por senha | Desde a tecla de executar | O tempo mede o banco, e não a digitação da senha |
| Abas da última sessão | Reabrir ao iniciar, com a opção `reopenTabs` e padrão ligado | Lista manual somente, como no plano original; perguntar ao iniciar | Reaberta pelo usuário em 2026-10-31: o texto já fica salvo, e a reabertura não executa SQL |
| Tabela da última sessão | Carregar na primeira seleção; uma aba que começa na tela espera `Enter` ou **Apply** | Carregar ao iniciar | A regra de conectar no primeiro uso continua: a abertura não conecta e não pede senha |
| Lista de abas após uma falha | Configuração inválida ou console bloqueado mantém a lista do arquivo | Gravar a lista da sessão atual | Uma sessão que não conseguiu reabrir as abas não pode apagar essa lista |
| Fechar aba com o mouse | `×` no fim de cada aba, com as perguntas de `Ctrl+W c` | Fechar sem perguntas; mostrar `×` só na aba ativa | Um clique não pode descartar uma transação ou um texto; o `×` em cada aba fecha outra aba sem trocar a tela |
| Comandos da linha de dicas | Tecla com o estilo das teclas do menu, efeito com a cor de dica e dois espaços entre os comandos | Separar com ` · ` | A cor marca o início de cada comando; ` · ` passa de 80 colunas no explorador, e dois espaços fecham em 80 |
| Duração das mensagens | 8 s; uma tecla, uma colagem ou um clique remove antes | Remover só com tecla; o ponteiro e a roda também removem | Uma mensagem sem prazo fica congelada para quem usa o mouse; mover o ponteiro não é uma resposta à mensagem |
| Espera de credenciais | A mensagem vem do estado da tentativa | Usar o mesmo prazo das outras mensagens | Uma espera longa no cofre continua ativa depois de 8 s, e o fim da espera deve remover a mensagem |
| Altura | Linha sob as abas e título do painel somente com 30 linhas ou mais | Sempre mostrar | Em 80 x 24, o editor e a grade mantêm as 18 linhas de antes |
| Tema sem cor | Mantém `[Run]`, sem ícones e sem `○` | Mesmos símbolos dos outros temas | Sem cor, o colchete identifica um botão |
| Tema ANSI | Sem fundo na base, no painel e na linha atual | Fundo preto | O perfil do terminal pode ser claro |
| Números do editor | Desenho próprio, com `LineNumbers::None` no EdTUI | Mudar o fork | O fork deve ficar pequeno até o lançamento |
| Segunda tecla da paleta | `Ctrl+W :` | Somente `Ctrl+P` | A regra 2 de `KEYMAP.md` pede outro caminho, e `Ctrl+P` não está no registro do `keytest` |
| Teclas na paleta | Uma tecla do explorador aparece somente com o explorador em foco | Mostrar sempre | Fora do explorador, essa letra vira texto |
| Rótulo da tecla | `Ctrl+P`, com a letra maiúscula | `Ctrl+p` | Uma letra sozinha mantém a caixa, pois `n` e `N` são teclas diferentes |
| Formulário de conexão | Grupos com quatro arranjos, do mais espaçado ao compacto, conforme a tela | Rolagem dentro da janela | Todos os campos ficam visíveis, e o teste de 80 x 24 continua válido |
| Foco no formulário | Marca, rótulo em destaque e fundo próprio | Somente o cursor | O usuário não via o campo selecionado |
| Arquivo SQLite no formulário | O texto digitado parte do diretório atual, e a conexão grava o caminho completo; um caminho salvo e não alterado fica como está | Resolver o texto digitado pelo escopo; converter também um caminho salvo | O usuário pede o caminho do teste. O teste usava o diretório atual, e a conexão salva abria outro arquivo. Editar outro campo não muda um caminho relativo escrito à mão |
| Arquivo SQLite em um projeto | Gravar também o caminho completo no `config.json` do projeto | Gravar relativo à raiz do projeto | É o pedido do usuário. Consequência: esse caminho vale somente nesta máquina; um caminho relativo escrito à mão continua válido |

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

O código implementa a etapa de credenciais, com texto simples autorizado, migração, senha recusada e exclusão separada.
O Keychain e o Secret Service em contêiner estão validados; falta repetir o Secret Service em um desktop Linux real.
A [matriz do plano](../PLAN.md#fechamento-da-etapa-de-credenciais) registra os critérios de conclusão.
Os testes com cofre em memória e bancos reais não substituem essa validação nativa.
O critério de cofre ausente vem da validação: somente respostas confirmadas no Linux, nunca no macOS.

## Credenciais e editor — 2026-10-31

**Definido:** o usuário aprova estes pontos depois da leitura do código.
A aprovação fecha as decisões, mas não declara as mudanças como implementadas.

| Ponto | Decisão aprovada | Rejeitado | Motivo |
|---|---|---|---|
| Keychain | Validar nesta máquina, com o usuário presente | Adiar | O texto simples depende dessa validação |
| Secret Service | Contêiner Linux agora e desktop real antes do lançamento | Somente desktop real | O contêiner cobre serviço ausente, bloqueio e operações comuns, mas não uma sessão de desktop |
| Senha em texto simples | `credentials.json` na pasta global, com `0600`, por identidade, para os dois escopos | `password` no `config.json` global e seção para conexões de projeto | `config.json` fica sem segredos, e os dois escopos usam um só formato |
| Fim da suspensão | Cada ação explícita abre a solicitação de senha; uma senha aceita ou a edição da conexão encerra a suspensão | Somente **Desconectar** ou edição | Execuções e catálogo param de insistir, sem um passo extra para o usuário |
| Limite do desfazer | Fork pequeno do `edtui` por `[patch]`; no lançamento, enviar ao projeto original ou publicar com outro nome | Copiar o código para `src/`; histórico próprio | A mudança fica em `Stack`; a cópia traz 10.169 linhas de terceiros, e o histórico próprio é frágil |

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
O código oferece sessão, cofre e a exceção global autorizada em texto simples, em `credentials.json`.

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
| Catálogo de exemplo | Somente nos testes, sem `--no-catalog` | Ativo por padrão no binário | Sem catálogo, a seção 6 pede palavras-chave; tabelas inventadas enganam o usuário |
| Publicação | Remover `publish = false` no lançamento | Manter, com somente binários | `cargo install sqln` depende do crates.io |
| Respostas de catálogo | Número em cada leitura; a resposta antiga sai | Aceitar qualquer resposta | Uma leitura antiga podia apagar do navegador uma tabela recém-criada |
| Teste de parada | Esperar o início real da instrução | Esperar 300 ms fixos | Sob carga, a conexão levava mais que isso, e o produto agia certo |

## Implementação — 2026-10-31

Estas escolhas seguem as regras do plano nos pontos que as regras deixaram abertos.

| Assunto | Escolha | Rejeitado | Motivo |
|---|---|---|---|
| Nova senha | Conferir com uma conexão de teste antes de gravar | Gravar e descobrir na próxima execução | Uma senha recusada nunca chega a um armazenamento |
| Primeira recusa | Não repetir a execução | Repetir sozinha depois da nova senha | Mais simples de explicar; a ação pedida durante a suspensão continua, pois nenhum SQL saiu |
| Último console | Console oculto, fora da fileira de abas | Permitir zero consoles | O código assume um console ativo; o usuário não vê diferença |
| Fim da última tabela | Encerrar a aplicação | Mostrar o console oculto | "Fechar a última aba" já é um caminho de saída |
| Troca de teclado | Mudar o modo no próprio editor | Recriar o editor e avisar | Nada se perde, então nenhum aviso é necessário |
| Relatório de panic | Local e pilha de chamadas, sem a mensagem | Incluir a mensagem | A mensagem pode citar SQL ou um valor |
| Log | Tipos de evento, sem texto de erro do banco | Registrar as mensagens | Um erro do banco pode citar um valor |
| Sinais | `SIGHUP`, `SIGINT`, `SIGQUIT` e `SIGTERM` | Somente `SIGTERM` | Fechar o terminal envia `SIGHUP` |
| Arquivo em texto simples | Recusar link e permissão aberta | Corrigir a permissão sozinho | O mesmo cuidado do `ssh` com uma chave privada |

## Temas — 2026-10-31

O usuário pediu temas, troca em tempo de execução e a escolha do tema padrão em Settings.
O tema por conexão, para identificar produção, fica para depois.

| Ponto | Decisão | Rejeitado | Motivo |
|---|---|---|---|
| Lista | Tokyo Night, Catppuccin Mocha, Catppuccin Latte, Gruvbox e Nord, além de Astrodark e `ansi` | Mais temas | Os tons diferem à primeira vista, o que ajuda o tema por conexão. Latte cobre quem prefere fundo claro |
| Definição | `Scheme` em `ui/schemes.rs`; `Theme::from_scheme` distribui as cores | Copiar os estilos para cada tema | Um tema novo não repete a lista de estilos |
| Ajustes de cor | Latte, Nord e Tokyo Night mudam alguns tons da paleta original | Paleta original sem mudança | Alguns tons originais não passam no contraste mínimo: 4,5 no texto e 3 em marcas e rótulos |
| Prévia | A escolha muda as cores antes de salvar, e cancelar restaura o tema salvo | Aplicar só ao salvar | O nome do tema não mostra as cores. A prévia também limita o dano em um terminal sem truecolor |
| Seção | `Appearance`, por último na lista | Campo em `General` | O tema por conexão e os ícones têm lugar natural nessa seção |
| Valores do arquivo | `kebab-case`, como `tokyo-night` | `lowercase` | `auto`, `astrodark` e `ansi` continuam válidos, e os nomes novos ficam legíveis |

### Tema por conexão

| Ponto | Decisão | Rejeitado | Motivo |
|---|---|---|---|
| Quem decide | A conexão da aba aberta, em console ou tabela, muda a tela inteira | Conexão selecionada no explorador; só a barra de status | O alerta de produção precisa acompanhar onde o comando roda. A seleção no explorador não executa nada |
| Sem tema próprio | A aba usa o tema padrão | Herdar o tema da aba anterior | A cor de uma aba não depende do caminho até ela |
| Ponto de escolha | `App::draw` calcula o tema a cada quadro, em `theme_choice` | Trocar o tema em cada mudança de aba | Há muitos caminhos de troca de aba. O cálculo em um só ponto não esquece nenhum, e a prévia não precisa de código próprio |
| Conexão ativa | O formulário permite mudar nome e tema (`display_only`), e o botão passa de `Save name` para `Save` | Exigir desconexão | O usuário precisa marcar produção com a sessão aberta. Tema não muda o acesso, então nenhuma sessão fecha |
| Recarga | `update_stored` aplica o tema mesmo com a conexão em uso | Adiar até desconectar | O arquivo e a tela ficam iguais, e a transação continua |
| Settings com tema próprio | A tela mostra o tema padrão escolhido enquanto o diálogo está aberto | Manter o tema da conexão | Sem isso, a prévia pareceria não funcionar em uma aba com tema próprio |
| Local no formulário | Último campo das opções | Junto do nome | Fica no fim do `Tab` e não muda a ordem dos campos de acesso |

Com os campos **Theme** e **Icon**, o formulário usa o modo compacto até 31 linhas. O modo com espaços entre grupos agora pede 32 linhas.

### Ícone por conexão

| Ponto | Decisão | Rejeitado | Motivo |
|---|---|---|---|
| Escolha | Lista fixa de 14 ícones no formulário | Campo de texto para colar um símbolo | O SQLN não consegue consultar a fonte do terminal. A lista garante um símbolo existente, e um símbolo colado poderia virar caixa vazia |
| Símbolos | Unicode de 1 coluna, conferidos com a Monaco no macOS, iguais em todos os conjuntos | Símbolos da Nerd Font | A Nerd Font é opcional, e sem ela esses símbolos viram caixa vazia. Estes símbolos existem com ou sem ela |
| Fora da lista | `⚡`, que vira emoji de 2 colunas, e `⛨` e `⚿`, mais largos que a célula | Aceitar qualquer símbolo Unicode | Um símbolo largo desalinha a linha do explorador |
| `NO_COLOR` | Mostra o ícone escolhido | Esconder, como os outros ícones | A escolha é do usuário, e o tema não identifica produção nesse modo |
| Marca do banco | Fica depois do nome, em todos os conjuntos | Sumir, porque o logo saiu do lugar | O ícone escolhido não diz qual é o banco |

### Limitações conhecidas

- Um tema RGB em um terminal sem truecolor pode mostrar cores erradas. A escolha explícita vence, porque `COLORTERM` falta em alguns terminais com suporte, como em uma sessão SSH.
- Com `NO_COLOR`, o campo muda o arquivo, mas não muda as cores. O tema por conexão não identifica produção nesse modo.
- Ao desconectar com o formulário aberto, ele recarrega a definição salva. Um nome ou tema ainda sem salvar volta ao valor salvo.

### Pendências

- Validar os temas e os ícones em um terminal real. A checagem visual usou as telas desenhadas com a fonte Monaco, fora do terminal.
