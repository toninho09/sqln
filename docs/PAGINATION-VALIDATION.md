# Verificação dos botões de página

Os botões ficam somente nas abas de tabela.
As decisões de `docs/DECISIONS.md` continuam em vigor.
O esboço de `docs/UI.md` mostra os botões e a barra de carga.

## Resultados

| Verificação | Resultado |
|---|---|
| `cargo test --tests`, com PostgreSQL 18 e MySQL 8.4 reais | 942 aprovados, 7 ignorados, nenhuma falha |
| `cargo clippy --all-targets -- -D warnings` | Sem avisos |
| `cargo test --test table`, em 10 rodadas consecutivas | 122 aprovados por rodada |
| Defeitos injetados na paginação | 17 detectados de 17; nenhuma falha de compilação |
| Binário release em PTY, com SQLite real | Nove verificações em cada um dos três layouts abaixo |
| `cargo test --release --test latency -- --nocapture --test-threads=1` | Cinco aprovados |
| Paginação com 50.000 linhas carregadas | Pior tecla mais quadro: 0,95 ms; meta: menos de 50 ms |

Os sete testes ignorados incluem cinco testes de latência do build de desenvolvimento.
Os cinco testes de latência passam no build release.

O teste de servidor verifica páginas carregadas, carga por **Next**, retorno, última página e **Load more**.
A verificação completa encontra uma expectativa antiga no teste de schema.
O teste agora procura o schema no título visível, não no botão que pode sair da barra.
Essa correção não altera o produto.

## Binário em PTY

Layouts: ANSI em 120×40, `astrodark` em 80×24 e `NO_COLOR` em 100×30.
O teste envia teclas e eventos de mouse SGR ao binário.
Um emulador de terminal interpreta os quadros recebidos.

As nove verificações cobrem a primeira página, **Next** por mouse, **Previous** por mouse e **Next** sobre dados carregados.
Também cobrem carga da última página, limite final, retorno por teclado, **Load more** sem movimento e página única sem botões.

## Defeitos injetados

Uma cópia temporária do código recebe um defeito por rodada.
A cópia sem defeitos passa nos 122 testes antes das injeções.
Os testes detectam cada alteração abaixo:

1. Consultar novamente ao avançar sobre dados carregados.
2. Não colocar a página no topo da grade.
3. Manter a página ao usar **Previous**.
4. Manter a página ao usar **Next**.
5. Remover o destino do pedido de página.
6. Remover o destino ao concluir a autorização.
7. Mover o cursor depois de uma carga interrompida.
8. Não mover o cursor depois de uma carga concluída.
9. Mover o cursor com **Load more**.
10. Mostrar botões para uma página única.
11. Esconder os botões antes da carga da segunda página.
12. Mostrar **Next** indisponível para dados carregados.
13. Mostrar **Previous** disponível na primeira página.
14. Tirar espaço do estado da carga para manter os botões longos.
15. Manter os botões durante a busca de linhas.
16. Fazer o clique em **Next** voltar uma página.
17. Desconsiderar a última página parcial.

## Limites

- O PTY não substitui um teste no iTerm real.
- Esta etapa não verifica fontes Nerd Font nem TLS dos servidores.
- O LSP não inicia porque falta `rust-analyzer` no toolchain local.
  Instale o componente com `rustup component add rust-analyzer`, ou ajuste o comando em `pi-lsp.json`.
