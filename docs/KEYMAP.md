# SQLN — Navegação e atalhos

## Estado deste documento

**Definido:** este documento está aprovado.
Ele detalha a navegação e os atalhos do [plano](../PLAN.md).
O plano continua sendo a referência das regras funcionais.

## Critérios de escolha

1. Usar teclas que o iTerm entregou no teste `keytest`.
2. Oferecer outro caminho para cada ação essencial.
3. Não depender somente de `Ctrl`+setas, `Home`, `End`, `PageUp` ou `PageDown`.
4. Manter as mesmas ações de execução nos modos convencional e Vim.

O módulo `input` aplica o campo `keymap`, e a aplicação o lê de `config.json`.
**Definido:** a barra, a ajuda `F1` e as opções do prefixo mostram as teclas efetivas do `keymap`.
Não mostrar atalhos removidos nem ações indisponíveis no painel atual.
O duplo clique em uma tabela e o clique no cabeçalho não foram testados em um terminal real.
`Shift+F5` e `Ctrl+Enter` foram testados com sequências simuladas em um PTY, não no iTerm.
As letras depois de `Ctrl+W` e `F1` também não foram testadas no iTerm.
Terminal.app e terminais do Linux também não foram testados.

## Regiões de foco

| Região | Função |
|---|---|
| Navegador | Conexões, objetos e Scripts sem conexão |
| Abas principais | Consoles e tabelas abertos |
| Editor | Texto SQL do console |
| Barra de filtro | Campos `WHERE` e `ORDER BY` da visão de tabela |
| Grade | Linhas do resultado ou da tabela |
| Abas de resultado | Resultados e **Mensagens** do console |
| Diálogo | Pergunta que bloqueia as demais regiões |

## Prioridade das teclas

A aplicação testa cada tecla nesta ordem e para na primeira correspondência:

1. Diálogo aberto.
2. Lista de sugestões aberta.
3. Prefixo `Ctrl+W` ativo.
4. Campo de texto em foco, como editor, filtro ou busca.
5. Atalhos do painel em foco.
6. Atalhos globais.

Durante a inserção de texto, um caractere sempre vira texto.
Uma tecla sem ação não produz efeito nem mensagem.

## Prefixo `Ctrl+W`

O prefixo funciona nos dois modos, sem depender de recursos do terminal.
Depois de `Ctrl+W`, pressione uma tecla. `Esc` cancela o prefixo.
Uma janela mostra as opções, em grupos, enquanto o prefixo estiver ativo.
A janela usa as teclas efetivas do `keymap`, e a barra de dicas mostra `Ctrl+W …  Esc cancel`.

| Tecla | Ação |
|---|---|
| `h`, `j`, `k`, `l` ou setas | Mudar de painel |
| `n` e `p` | Próxima aba e aba anterior |
| `N` | Renomear o console atual |
| `a` | Trocar ou associar a conexão do console |
| `,` | Abrir Configurações |
| `c` | Fechar a aba |
| `q` | Sair da aplicação |
| `b` | Recolher ou mostrar o navegador |
| `e` | Executar |
| `s` | Script inteiro |
| `x` | Parar o script ou a consulta da tabela |
| `m` | Trocar entre autocommit e manual |
| `C` | Commit |
| `R` | Rollback |
| `d` | Escolher banco ou schema |
| `r` | Ler a configuração de novo |
| `?` | Abrir a ajuda |
| `:` | Abrir a paleta de comandos |

`Ctrl+W C` e `Ctrl+W R` não pedem confirmação, como os botões de commit e rollback.
`Ctrl+W N` usa `N` maiúsculo e não substitui `Ctrl+W n`.
A lista de consoles oferece a ação de renomear com `r`.
A troca de conexão exige confirmação e não executa SQL. As regras estão na seção 12 do plano.
As abas principais seguem a ordem de abertura, sem grupos por tipo.
Fechar o último console com uma tabela aberta não cria outra aba de console.

No modo convencional, `Ctrl+W` não fecha a aba, ao contrário de muitas IDEs.
O prefixo é o caminho comum aos dois modos, e o terminal entrega essa tecla.

## Ações globais

| Ação | Atalho | Alternativa |
|---|---|---|
| Executar | `Ctrl+Enter` | `F5` ou `Ctrl+W e` |
| Script inteiro | `Shift+Enter` | `Shift+F5` ou `Ctrl+W s` |
| Parar script | `Ctrl+W x` | Botão **Parar script** |
| Ajuda | `F1` | `Ctrl+W ?` |
| Paleta de comandos | `Ctrl+P` | `Ctrl+W :` |
| Sair | `Ctrl+W q` | Fechar a última aba |

`Ctrl+Enter` e `Shift+Enter` exigem aprimoramento de teclado no terminal.
Ao iniciar, a aplicação detecta esse recurso e mostra na barra de dicas o caminho disponível.

A paleta lista as ações disponíveis no estado atual, com a tecla de cada uma.
Digite para filtrar. `Up` e `Down` mudam a linha, `Enter` executa, `Esc` fecha, e um clique executa a linha.
Uma ação sem tecla continua na lista. Uma tecla do explorador aparece somente com o explorador em foco.
`Ctrl+P` tem prioridade sobre o `MoveUp` do EdTUI na inserção do modo Vim.

`Ctrl+C` não encerra a aplicação nem para o script.
Com seleção, ele copia. Sem seleção, ele não faz nada.

## Editor

| Ação | Convencional | Vim |
|---|---|---|
| Modo de inserção | Sempre ativo | `i`, `a` e `o` |
| Sair da inserção | Sem efeito | `Esc` |
| Desfazer | `Ctrl+Z` | `u` |
| Refazer | `Ctrl+Y` ou `Ctrl+Shift+Z` | `Ctrl+R` |
| Selecionar | `Shift`+setas | `v` |
| Selecionar tudo | `Ctrl+A` | `ggVG` |
| Copiar, recortar e colar | `Ctrl+C`, `Ctrl+X` e `Ctrl+V` | `y`, `d` e `p` |
| Mover por palavra | `Alt`+setas | `w` e `b` |
| Buscar | `Ctrl+F` | `/` |
| Substituir | `Ctrl+F`, depois `Tab` | `Ctrl+F`, depois `Tab` |
| Sugestões | `Ctrl+Space` | `Ctrl+Space` |
| Gravar agora | `Ctrl+S` | `Ctrl+S` |

A lista abre ao digitar um nome ou um ponto, e `Ctrl+Space` abre a lista sem prefixo.
No modo Vim, a lista funciona somente na inserção.
Com sugestões abertas, `Tab` aceita, `Enter` quebra a linha e `Esc` fecha a lista.
A opção **Autocomplete** do diálogo Settings (`suggestionKeys`) troca a tecla que aceita: `tab` (padrão), `enter` ou `both`. A tecla que não aceita mantém o seu efeito.
As setas percorrem a lista enquanto ela estiver aberta.

Na barra de busca:

| Ação | Tecla |
|---|---|
| Mudar de campo | `Tab` e `Shift+Tab` |
| Próximo resultado | `Enter` no campo de busca, ou `Down` |
| Resultado anterior | `Up` |
| Substituir o resultado atual | `Enter` no campo de substituição |
| Substituir todos | `Ctrl+A` no campo de substituição |
| Fechar a barra | `Esc` |

No macOS, o terminal trata `Cmd+C` e `Cmd+V`.
`Cmd+V` chega à aplicação como uma colagem.
`Cmd+C` copia a seleção do terminal, não a seleção do editor.

## Navegador

| Ação | Convencional | Vim |
|---|---|---|
| Mover | Setas | `j` e `k` |
| Recolher e expandir | `Esquerda` e `Direita` | `h` e `l` |
| Abrir | `Enter` | `Enter` |
| Novo console | `n` | `n` |
| Atualizar catálogo | `r` | `r` |
| Desconectar | `D` | `D` |
| Adicionar conexão | `a` | `a` |
| Editar conexão | `e` | `e` |
| Excluir, com confirmação | `Delete` | `x` |
| Scripts sem conexão | `s` | `s` |

`Enter` em uma conexão seleciona o primeiro console aberto dela ou cria um console.
`n` abre a lista de consoles da conexão, com **+ New console** na primeira linha. Sem consoles salvos, `n` cria um console.
Na lista, `Enter` escolhe a linha, `Delete` (ou `x` no modo Vim) remove o script da linha, e `Esc` cancela.
`s` abre a lista de scripts sem conexão, com as mesmas teclas.
Em um schema, `Enter` abre ou fecha a linha.
Em uma tabela ou uma view, `Enter` abre a visão de tabela, ou seleciona a aba dela, quando já existe.
`Direita` e `Esquerda` abrem e fecham as colunas de uma tabela.

## Formulário de conexão

| Ação | Tecla |
|---|---|
| Próximo campo | `Tab`, `Down` ou `Enter` em um campo |
| Campo anterior | `Shift+Tab` ou `Up` |
| Mudar uma opção | `Left` e `Right`; no modo Vim, também `h` e `l` |
| Marcar **Somente leitura** | `Space`, `Left` ou `Right` |
| Acionar um botão | `Enter` |
| Cancelar | `Esc` |

Em um campo de texto, toda letra vira texto.
No modo Vim, `j` e `k` mudam de campo somente fora dos campos de texto.

## Solicitação de senha

A senha fica mascarada, inclusive durante colagem. O salvamento começa desmarcado.

| Ação | Tecla |
|---|---|
| Informar senha | Digitar ou colar no campo |
| Próximo campo | `Tab` ou `Down` |
| Campo anterior | `Shift+Tab` ou `Up` |
| Marcar salvamento no cofre | `Space` ou `Enter` na opção |
| Conectar | `Enter` no campo de senha ou no botão **Connect** |
| Cancelar sem iniciar a operação | `Esc` ou botão **Cancel** |

Fora do campo de senha, as setas horizontais mudam o foco.
No modo Vim, `h/k` e `j/l` também mudam o foco fora do campo.
Dentro do campo, essas letras sempre viram texto.
O mouse seleciona o campo, marca a opção ou aciona um botão.
Se o cofre falhar, a opção de sessão não oferece gravação global em texto simples.

## Grade

| Ação | Convencional | Vim |
|---|---|---|
| Mover | Setas | `h`, `j`, `k` e `l` |
| Primeira e última linha | `Alt+Up` e `Alt+Down` | `gg` e `G` |
| Primeira e última coluna | `Alt+Left` e `Alt+Right` | `0` e `$` |
| Selecionar | `Shift`+setas | `v` |
| Selecionar tudo | `Ctrl+A` | `ggVG` |
| Copiar em TSV | `Ctrl+C` | `y` |
| Limpar a seleção | `Esc` | `Esc` |
| Abrir valor | `Enter` | `Enter` |
| Buscar nas linhas carregadas | `Ctrl+F` | `/` |
| Diminuir e aumentar a coluna | `<` e `>` | `<` e `>` |
| Largura automática da coluna | `=` | `=` |
| Fechar o resultado | `x` ou `Delete` | `x` ou `Delete` |
| Resultado anterior e próximo | `[` e `]` | `[` e `]` |

Sem seleção, `Ctrl+C` copia a célula atual.
O resultado que ainda recebe linhas não fecha, e a aba **Mensagens** também não.
Na aba **Mensagens**, as setas percorrem o texto.

Na busca da grade:

| Ação | Tecla |
|---|---|
| Próximo resultado | `Enter` ou `Down` |
| Resultado anterior | `Up` |
| Fechar a barra | `Esc` |

Na visão de tabela, a grade também oferece:

| Ação | Tecla |
|---|---|
| Foco em `WHERE` | `f` |
| Foco em `ORDER BY` | `o` |
| Ordenar pela coluna atual | `s` |
| Carregar mais | `m` |

## Barra de filtro

| Ação | Tecla |
|---|---|
| Aceitar sugestão | `Tab`, ou a tecla da opção **Autocomplete** |
| Aplicar os campos | `Enter` |
| Mudar de campo | `Shift+Tab`, ou `Tab` sem sugestões |
| Abrir as sugestões | `Ctrl+Space`, ou ao digitar um nome |
| Fechar as sugestões | `Esc` |
| Voltar à grade | `Esc`, com as sugestões fechadas |

`Ctrl+W k` vai ao campo `WHERE`, e `Ctrl+W j` volta à grade.
Na visão de tabela, as ações de script e de transação não têm efeito, pois a tabela não tem script nem transação.
Essas ações são `F5`, `Shift+F5`, `Ctrl+W e`, `Ctrl+W s`, `Ctrl+W m`, `Ctrl+W C`, `Ctrl+W R` e `Ctrl+W d`.
Na grade da tabela, `x`, `Delete`, `[` e `]` também não têm efeito.
`Ctrl+W c` fecha a aba da tabela e libera seus dados.

## Pergunta sobre scripts alterados

| Ação | Tecla |
|---|---|
| Mudar de linha ou de botão | `Up`, `Down`, `Tab` e `Shift+Tab`; no modo Vim, também `j` e `k` |
| Mudar a ação de uma linha | `Left`, `Right` e `Space`; no modo Vim, também `h` e `l` |
| Escolher a ação e ir à linha seguinte | `Enter` |
| Aplicar, com o foco em **Apply** | `Enter` |
| Decidir depois | `Esc`, ou `Enter` em **Decide later** |

**Aplicar** exige uma ação em cada linha.

## Diálogos

| Ação | Tecla |
|---|---|
| Mudar de opção | `Tab` e `Shift+Tab`; no modo Vim, também `h` e `l` |
| Confirmar a opção em foco | `Enter` |
| Cancelar | `Esc` |

O foco inicial fica sempre na opção sem efeito no banco ou no disco.
`Enter` nunca aciona commit, rollback ou exclusão sem foco explícito nessa opção.

## Mouse

| Gesto | Efeito |
|---|---|
| Clique | Selecionar um item e mudar o foco |
| Clique em uma aba | Trocar de aba |
| Clique no `×` de uma aba | Fechar a aba, com as perguntas de `Ctrl+W c` |
| Clique no título `EXPLORER` | Dar o foco ao explorador |
| Clique no cabeçalho da tabela | Aplicar a ordenação: crescente, decrescente e sem ordem |
| Duplo clique em uma tabela | Abrir a visão de tabela, com dois cliques na mesma célula em 400 ms |
| Clique em um campo ou botão da barra | Dar o foco ao campo, ou acionar o botão |
| Clique em uma linha da pergunta sobre scripts | Mudar a ação da linha, ou acionar o botão |
| Roda | Percorrer listas, editor e grade |
| Arrasto em um divisor | Ajustar os painéis |
| Arrasto na borda de um cabeçalho | Ajustar a largura da coluna |
| Arrasto na grade | Selecionar células |

Um clique também responde a um diálogo e aciona os botões da barra do console.
Um clique em qualquer lugar também remove a mensagem da linha de dicas.
A linha das abas de resultado serve de divisor entre o editor e os resultados.
O teste `keytest` não registrou um arrasto.
Com captura de mouse ativa, a seleção nativa do terminal exige `Option` no iTerm.

## Terminal

1. Pedir somente a desambiguação de teclas, não o relato de todas as teclas.
2. Ignorar eventos de teclas modificadoras isoladas.
3. Tratar `Event::Paste` como colagem.
4. Restaurar o terminal ao sair, inclusive depois de um panic.

## Configuração

O campo `keymap` de `config.json` associa uma ação a uma lista de teclas, por modo.

```json
{
  "keymap": {
    "conventional": { "run": ["ctrl+enter", "f5", "ctrl+w e"], "stop": [] },
    "vim": { "run": ["ctrl+enter", "f5"] }
  }
}
```

Uma lista substitui as teclas padrão da ação, e uma lista vazia remove o atalho.
Uma tecla pode ter duas partes separadas por espaço, como `ctrl+w e` e `g g`.
Duas ações com a mesma tecla, na mesma região, invalidam a configuração.
Uma tecla e uma sequência que começa com ela também invalidam a configuração.
Um nome de ação ou de tecla desconhecido também invalida a configuração.
Nesse caso, valem as regras de configuração inválida do plano.
