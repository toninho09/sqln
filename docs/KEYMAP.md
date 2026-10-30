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

`F1`, `Shift+F5` e as letras depois de `Ctrl+W` ainda não foram testados com a aplicação.
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
A barra de dicas mostra as opções enquanto o prefixo estiver ativo.

| Tecla | Ação |
|---|---|
| `h`, `j`, `k`, `l` ou setas | Mudar de painel |
| `n` e `p` | Próxima aba e aba anterior |
| `c` | Fechar a aba |
| `b` | Recolher ou mostrar o navegador |
| `e` | Executar |
| `s` | Script inteiro |
| `x` | Parar o script ou a consulta da tabela |
| `m` | Trocar entre autocommit e manual |
| `C` | Commit |
| `R` | Rollback |
| `d` | Escolher banco ou schema |
| `?` | Abrir a ajuda |

`Ctrl+W C` e `Ctrl+W R` não pedem confirmação, como os botões de commit e rollback.

No modo convencional, `Ctrl+W` não fecha a aba, ao contrário de muitas IDEs.
O prefixo é o caminho comum aos dois modos, e o terminal entrega essa tecla.

## Ações globais

| Ação | Atalho | Alternativa |
|---|---|---|
| Executar | `Ctrl+Enter` | `F5` ou `Ctrl+W e` |
| Script inteiro | `Shift+Enter` | `Shift+F5` ou `Ctrl+W s` |
| Parar script | `Ctrl+W x` | Botão **Parar script** |
| Ajuda | `F1` | `Ctrl+W ?` |
| Sair | Fechar a última aba | Comando de saída pela interface |

`Ctrl+Enter` e `Shift+Enter` exigem aprimoramento de teclado no terminal.
Ao iniciar, a aplicação detecta esse recurso e mostra na barra de dicas o caminho disponível.

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

Com sugestões abertas, `Tab` aceita, `Enter` quebra a linha e `Esc` fecha a lista.
As setas percorrem a lista enquanto ela estiver aberta.

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
| Editar conexão | `e` | `e` |
| Excluir, com confirmação | `Delete` | `x` |

## Grade

| Ação | Convencional | Vim |
|---|---|---|
| Mover | Setas | `h`, `j`, `k` e `l` |
| Primeira e última linha | `Alt+Up` e `Alt+Down` | `gg` e `G` |
| Primeira e última coluna | `Alt+Left` e `Alt+Right` | `0` e `$` |
| Selecionar | `Shift`+setas | `v` |
| Selecionar tudo | `Ctrl+A` | `ggVG` |
| Copiar em TSV | `Ctrl+C` | `y` |
| Abrir valor | `Enter` | `Enter` |
| Buscar nas linhas carregadas | `Ctrl+F` | `/` |
| Resultado anterior e próximo | `[` e `]` | `[` e `]` |

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
| Aceitar sugestão | `Tab` |
| Aplicar os campos | `Enter` |
| Mudar de campo | `Shift+Tab`, ou `Tab` sem sugestões |
| Voltar à grade | `Esc` |

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
| Clique no cabeçalho da tabela | Aplicar a ordenação |
| Duplo clique em uma tabela | Abrir a visão de tabela |
| Roda | Percorrer listas, editor e grade |
| Arrasto em um divisor | Ajustar os painéis |
| Arrasto na grade | Selecionar células |

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
    "conventional": { "run": ["ctrl+enter", "f5"], "stop": [] },
    "vim": { "run": ["ctrl+enter", "f5"] }
  }
}
```

Uma lista vazia remove o atalho da ação.
Duas ações com a mesma tecla, na mesma região, invalidam a configuração.
Nesse caso, valem as regras de configuração inválida do plano.
