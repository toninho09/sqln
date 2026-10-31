# SQLN — Esboços das telas

## Estado do desenho

Este documento apresenta uma proposta visual baseada no [plano funcional](../PLAN.md).
O plano continua sendo a referência das decisões confirmadas.

Os valores, nomes e dados abaixo são exemplos fictícios.
Tamanhos, espaçamentos e cores ainda são propostas visuais.
Os atalhos aprovados estão em `docs/KEYMAP.md`.
O usuário aprova os fluxos e ajustes registrados em `docs/DECISIONS.md` em 2026-10-31.
Essa aprovação não significa que todos os fluxos já existem no código.
A interface final usa inglês; os rótulos em português destes esboços são exemplos.
A aplicação implementa as seções 1 a 4 e 6, e parte das seções 5 e 8.
O cadastro já oferece **Session only** e **System vault**. Migração e texto simples autorizado ainda não existem.
A tela de abertura da seção 1 é uma lista sobre a tela, e não um painel com busca.
O diálogo **Configurações** da seção 8 ainda não existe. A [seção 1 do plano](../PLAN.md) lista o que falta.

## 1. Inicialização e abertura de console

Se não encontrar `.sqln/` no diretório atual ou nos superiores, perguntar sobre sua criação.
Aceitar cria o projeto no diretório atual. Recusar mantém somente o escopo global.

```text
┌─ Criar projeto ─────────────────────────────────────┐
│ Nenhuma pasta .sqln/ foi encontrada.                 │
│ Criar um projeto no diretório atual?                │
│                                                    │
│ [Criar projeto]  [Usar somente configurações globais]│
└────────────────────────────────────────────────────┘
```

A tela de abertura oferece os consoles da conexão selecionada, sem restaurá-los automaticamente.

Implementação atual: `n` no navegador abre uma lista sobre a tela, com o título `Consoles of <conexão> · <escopo>`.
A primeira linha, **+ New console**, cria um console. As outras linhas mostram o nome, a idade da última alteração e a marca `(open)`.
A lista não tem campo de busca nem botões. `Enter` escolhe, `Delete` remove, e `Esc` cancela.
`s` no navegador abre a lista **Scripts without a connection**, que mostra também o escopo de cada script.

```text
┌─ Conexões ──────────┬─ Consoles de app (local) ──────────────┐
│ app (local)        │ Buscar: [                             ]│
│ app (global)       │                                        │
│ warehouse          │ console-1                hoje, 10:30    │
│                    │ investigar-pedidos       ontem, 16:20   │
│                    │                                        │
│                    │ [Abrir]  [Criar novo console]           │
└────────────────────┴────────────────────────────────────────┘
```

O campo de busca e a apresentação de horários são propostas de desenho.
A filtragem por conexão e a oferta de reutilização já estão definidas.

Abrir um console já aberto seleciona sua aba existente.
Criar outro console gera um nome automático, que pode ser alterado depois.
**Definido, ainda não implementado:** `Ctrl+W N` renomeia o console atual; a lista também oferece a ação.
Recusar nomes vazios ou repetidos na mesma conexão, sem mudar identidade, arquivos, conexão ou escopo.

**Definido, ainda não implementado:** `Ctrl+W a` abre a troca ou associação de conexão do console.
Bloquear durante execução ou transação aberta.
Mostrar a conexão atual, a nova conexão e a pasta de destino antes da confirmação.
Salvar antes da troca; uma falha cancela a troca.
Preservar texto, nome, cursor e rolagem; limpar resultados, mensagens e seleção de banco ou schema.
Não executar nem converter o SQL automaticamente.

## 2. Visão de console

Editor e resultados aparecem juntos.
O usuário pode arrastar o divisor e recolher o navegador de conexões.

```text
┌─ Projeto: loja ───────────────────────────────────────────────────┐
│ Conexões           │ console-1  │ users [tabela]                  │
├────────────────────┼─────────────────────────────────────────────┤
│ app (local)        │ app_db@db.example.test · TLS verificado      │
│   public           │ Schema: [public]                             │
│     users          │ [Executar] [Script inteiro] [Parar script]  │
│     orders         │ Modo: [Auto] · Commit/rollback indisponíveis │
│ app (global)       ├─────────────────────────────────────────────┤
│ warehouse          │ 1 ✓ SELECT 1;                               │
│                    │ 2                                           │
│                    │ 3 ✓ SELECT id, name                         │
│                    │ 4   FROM public.users;                      │
│                    ├─ Resultado 1 ─ [Resultado 2] ─ Mensagens ───┤
│                    │ id │ name                                   │
│                    │  1 │ Ana                                    │
│                    │  2 │ Bruno                                  │
│                    │ ...                                         │
│                    │ 1.000 linhas lidas · teto atingido          │
│                    │ O resultado pode ter mais linhas.           │
├────────────────────┴─────────────────────────────────────────────┤
│ Convencional · Pool ocupado: 0/5 · Dados: 1/256 MiB              │
└──────────────────────────────────────────────────────────────────┘
```

Os indicadores ficam na coluna livre, entre o número da linha e o texto:

| Símbolo | Estado |
|---|---|
| `●` | Executando |
| `✓` | Sucesso |
| `⋯` | Teto ou orçamento atingido |
| `✗` | Erro |
| `—` | Não executada |
| `■` | Parada |
| `◷` | Timeout |
| `?` | Resultado desconhecido |

Quando o banco informa a posição de um erro, o editor sublinha o token e mantém o cursor.
A marca sai com a próxima edição, e uma edição durante a execução impede a marca.

Esse exemplo mostra uma execução concluída, com duas instruções.
A segunda instrução atingiu o teto de linhas e encerrou a leitura.
A conexão já voltou ao pool, e o usuário percorre as linhas sem nova consulta.
Não mostrar sucesso definitivo de uma instrução antes de terminar sua leitura.

Regras dos controles:

- **Executar:** usa a seleção ou, sem seleção, a instrução sob o cursor.
- **Script inteiro:** executa o texto completo, parando no primeiro erro.
- **Parar script:** interrompe a sequência sem fechar o console, apagar o texto ou resolver a transação automaticamente.
- Não enviar as próximas instruções após aceitar a parada; não prometer interrupção imediata no servidor.
- Parar uma instrução ativa envia o cancelamento e aguarda 5 segundos pela resposta.
- Sem resposta, fechar a conexão; com transação aberta, pedir confirmação antes de fechar.
- Depois de fechar, indicar que o resultado da instrução é desconhecido.
- O seletor de banco ou schema vale para cada execução, mesmo com outra conexão física.
- Aceitar uma nova execução limpa as abas de resultados anteriores desse console, sem apagar o texto ou resolver a transação.
- A limpeza ocorre mesmo quando a nova execução precisa aguardar conexão; uma tentativa bloqueada não limpa os resultados.
- Cada resultado lê linhas até o teto, sem aguardar uma ação do usuário.
- Um resultado sem linhas mostra as colunas e a indicação de zero linhas.
- Ao atingir o teto, informar que o resultado pode ter mais linhas.
- Para ver mais linhas, aumentar o teto e executar o comando de novo.
- Bloquear outra execução, troca de modo e botões de commit ou rollback enquanto a execução estiver pendente ou ativa.
- Trocar o modo pelo seletor da barra ou pelo atalho.

O timeout SQL padrão é de 30 segundos, configurável, com opção de executar sem prazo.
Ele não substitui o prazo de conexão nem limita a espera no pool.
Contar o prazo por instrução, descontando a espera no pool e incluindo a leitura das linhas.

Depois da execução, uma transação aberta mantém sua conexão reservada.
Nesse estado, o usuário pode iniciar outra execução ou resolver a transação pelos controles disponíveis.

## 3. Visão de tabela

Duplo clique na tabela abre uma aba na mesma faixa dos consoles.
Abrir novamente a mesma tabela seleciona essa aba, sem duplicar.

```text
┌─ Conexões ─────────┬─ console-1 ─ [users: tabela] ────────────────┐
│ app (local)       │ app_db.public.users                          │
│   public          │ WHERE    [active = true                    ] │
│     users         │ ORDER BY [                                 ] │
│     orders        │ [Aplicar] [Limpar]                           │
│ app (global)      ├────┬──────────┬──────────────────────────────┤
│ warehouse         │ id │ name     │ active                       │
│                   │  1 │ Ana      │ true                         │
│                   │  2 │ Bruno    │ true                         │
│                   │ ...                                         │
│                   │ 100 linhas carregadas · [Carregar mais]      │
├───────────────────┴─────────────────────────────────────────────┤
│ App (local) · Dados das grades: 1/256 MiB                          │
└─────────────────────────────────────────────────────────────────┘
```

O exemplo mostra um filtro que o usuário já aplicou.
Manter filtros e ordenação somente enquanto essa aba permanecer aberta.
Fechar a aba descarta esses valores; reabrir começa sem filtros nem ordenação.
Não restaurar abas de tabela automaticamente ao reiniciar a aplicação.
Uma tabela nova não recebe ordenação automática, nem por chave primária.
Não incluir uma aba de estrutura nesta versão.

Regras da barra:

- Digitar não executa consultas.
- Recusar um filtro com outra instrução.
- `Ctrl+W x` para uma consulta da tabela em andamento.
- `Tab` aceita uma sugestão de autocomplete.
- `Enter` aplica o conteúdo dos campos, mesmo com autocomplete aberto.
- `[Aplicar]` tem o mesmo efeito de `Enter`.
- `[Limpar]` limpa os campos e aplica a consulta sem filtro nem ordenação.
- Clique no cabeçalho aplica a ordenação imediatamente, mantendo o `WHERE` já aplicado.
- Não aplicar um rascunho de `WHERE` como efeito do clique no cabeçalho.
- Filtro ou ordenação inválidos deixam a grade vazia, com o erro e os campos disponíveis para correção.

Implementação atual, de cima para baixo: título, campo `WHERE`, campo `ORDER BY`, botões
`[Apply] [Clear] [Load more] [Stop]`, grade, resumo e uma linha de aviso.
O rótulo da aba é `nome (table)` ou `nome (view)`.
**Definido:** a faixa mistura consoles e tabelas na ordem de abertura.
Fechar o último console com uma tabela aberta seleciona uma tabela, sem criar um console vazio.
O código ainda agrupa consoles antes das tabelas e cria um console vazio ao fechar o último.
O resumo mostra as linhas carregadas e o estado.
A linha de aviso mostra o risco da ordem instável, ou a mensagem da última consulta.
O erro de uma primeira página ocupa a área da grade.
O clique no cabeçalho ordena em ordem crescente, depois decrescente, e depois remove a ordem.

Rolagem e navegação percorrem os dados já carregados.
Mais linhas exigem a ação **Carregar mais**.
Cada página faz uma nova consulta, sem manter uma leitura aberta entre páginas.
**Carregar mais** pode aguardar uma conexão livre.
Sem uma ordenação única, linhas podem repetir ou faltar entre as páginas.
Mostrar esse aviso quando não existir uma ordenação aplicada.
O teto global de 256 MiB pode suspender novas cargas, sem apagar as linhas existentes.
Se um valor ultrapassar o limite, indicar o motivo sem representá-lo como nulo, vazio ou conteúdo truncado sem aviso.
A cópia segue o formato TSV confirmado na seção 10 do plano, sem cabeçalhos.

## 4. Cadastro de conexão

Oferecer formulário e entrada por URI, com a mesma política de credenciais.
Com projeto ativo, selecionar projeto por padrão e permitir escolher global.
Sem projeto ativo, oferecer somente global.
O exemplo abaixo considera um projeto ativo.

```text
┌─ Conexão ──────────────────────────────────────────────┐
│ Tipo:       [PostgreSQL]                               │
│ Nome:       [app]                                      │
│ Escopo:     [Projeto]  (pode alterar para Global)       │
│ Entrada:    [Campos / URI]                             │
│                                                       │
│ Host:       [db.example.test]                          │
│ Porta:      [5432]                                     │
│ Banco:      [app_db]                                   │
│ Usuário:    [app_user]                                 │
│ Credencial: [Somente nesta sessão]                    │
│ TLS:        [Verificar certificado]                    │
│ CA:         [Do sistema]                               │
│ Certificado de cliente: [Nenhum]                       │
│ Pool:       [5]                                        │
│ Somente leitura: [ ]                                   │
│ Conectar:   prazo [30] segundos                        │
│                                                       │
│ [Testar conexão]  [Salvar]  [Cancelar]                │
└───────────────────────────────────────────────────────┘
```

Os campos mudam conforme o tipo. SQLite usa arquivo, não banco em memória.
Resolver caminhos relativos conforme o escopo: raiz do projeto ou pasta global de configuração.

O formulário sugere TLS desativado em `localhost`, `127.0.0.0/8`, `::1` e socket Unix, e verificação nos demais.
O campo **Somente leitura** é aplicado pelo banco e evita acidentes.

Exigir nome obrigatório e único por escopo, sem distinguir maiúsculas e minúsculas, inclusive ao renomear.
Permitir nomes iguais entre global e projeto, com indicação de origem quando houver colisão.

Exigir desconexão antes de alterar dados de acesso, TLS ou limite do pool de uma definição conectada.
Uma URI não autoriza salvar senha na configuração do projeto.
Não mostrar senhas em exemplos, mensagens de erro ou representações públicas da conexão.

## 5. Confirmações e erros

### Transação aberta

Ao fechar um console com transação aberta, oferecer commit, rollback ou cancelamento do fechamento.
Na saída da aplicação, perguntar separadamente para cada console.
Executar a escolha antes de apresentar a pergunta do próximo console.
Cancelar depois não desfaz operações já concluídas.

Ao ativar autocommit com transação aberta, usar as mesmas três decisões para concluir ou cancelar a troca.
Não selecionar commit como consequência automática da solicitação de troca.

Se o commit ou rollback do fechamento falhar, manter o console aberto e mostrar o erro.
Oferecer nova tentativa, quando a sessão original e o estado da transação permitirem, ou cancelar o fechamento.
Não repetir a operação automaticamente nem usar outra conexão para tentar concluir a transação anterior.
Se perder a conexão sem confirmar o resultado, indicar que o resultado é desconhecido.
Durante a saída, resolver a falha atual antes de perguntar sobre outro console.

### Conexão

- Conectar no primeiro uso, sem abrir conexões ao iniciar a aplicação.
- Oferecer a ação **Desconectar** em cada definição conectada.
- Antes da primeira conexão a um destino nesta máquina, mostrar host, porta, usuário e TLS.
- Perguntar de novo quando um desses valores mudar.
- Após perder uma transação aberta, marcar o estado como desconhecido e exigir confirmação antes da próxima execução.

### Leitura e fechamento

- Fechar um console ativo pede confirmação para parar a sequência após finalizar a instrução atual.
- Não permitir fechar um resultado enquanto sua instrução ainda lê linhas; usar **Parar script**.
- Não prometer cancelamento imediato do comando em andamento no servidor.

### Arquivo e configuração

- Excluir um console salvo ou uma conexão exige confirmação dos itens afetados.
- Fechar uma aba não exclui seu script salvo.
- Remover uma conexão preserva seus scripts em disco, sem exclusão em cascata ou reassociação automática por nome.
- Mostrar esses scripts na lista Scripts sem conexão, permitindo abrir e editar sem executar SQL.
- Exigir escolha e confirmação de uma conexão antes de executar; associar não executa SQL automaticamente.
- Se a associação mudar o escopo, informar a nova localização do arquivo antes de concluir.
- Falha de salvamento mantém o texto em memória e mostra um alerta.
- Fechar com alterações não gravadas exige confirmação explícita.
- Alteração externa do script exige escolha entre recarregar e sobrescrever.
- Uma configuração JSON inválida mostra arquivo e posição do erro, sem liberar operações com bancos até a correção.
- O usuário corrige esse arquivo fora da aplicação e usa **Recarregar**.
- **Definido, ainda não implementado:** `workspace.json` ilegível usa o estado inicial e mostra um aviso não bloqueante.
- **Definido, ainda não implementado:** a ação para um script removido usa o rótulo **Fechar e descartar texto**.

### Credenciais

Usar o cofre como armazenamento principal.
Somente a ausência de cofre permite oferecer gravação global em texto simples, mediante autorização explícita.
Cofre bloqueado ou com erro não ativa essa exceção.
Com o cofre bloqueado ou com erro, oferecer **Tentar novamente**, **Usar senha nesta sessão** e **Cancelar**.
Tratar uma falha desconhecida como erro, nunca como ausência.
Confirmar a distinção em testes reais antes de liberar texto simples como alternativa.
Senha de sessão permanece em memória, por conexão, até fechar a aplicação.
Se a gravação falhar, manter a senha em memória, informar que ela não foi salva e oferecer nova tentativa explícita.
Quando um cofre passar a existir, pedir confirmação para migrar e verificar a gravação antes de oferecer remover a senha global.
Falha na migração não autoriza apagar a senha anterior.

Usar identidades estáveis ao mover um projeto.
Pedir confirmação antes de reutilizar credenciais em cópias ou quando não for possível distinguir cópia de mudança de pasta.
Em outro computador sem a credencial, solicitar a senha, sem transportar segredos pelo projeto.
Mudar o destino não autoriza enviar a senha anterior automaticamente.
Remover uma conexão não apaga sua senha do cofre; excluir essa senha exige confirmação separada.
Avisar que outras cópias do projeto podem usar a mesma credencial.

Solicitar a senha antes do primeiro uso que precisar dela, inclusive teste e leitura do catálogo.
Iniciar com uso somente na sessão. Salvar exige uma escolha explícita.
A solicitação de senha, a confiança e a recuperação de falhas do cofre já existem no código.
A senha fica mascarada, e a opção de salvar começa desmarcada.
O formulário permite sessão ou cofre; testar uma senha digitada não a grava nem substitui a senha da conexão salva.
Um caminho novo de projeto exige confirmação também ao testar ou salvar uma definição existente.
Faltam texto simples autorizado, migração, senha recusada e exclusão separada pela interface.

#### Fluxos de credenciais ainda não implementados

Estes fluxos seguem regras já aprovadas. Os detalhes abaixo não declaram as telas como implementadas.

| Fluxo | Conteúdo necessário | Regra de segurança |
|---|---|---|
| Texto simples global | Aviso antes da autorização, opção desmarcada e identificação do armazenamento global | Oferecer somente com ausência confirmada de cofre; nunca gravar o segredo no projeto |
| Migração | Confirmação para migrar e, após conferência, oferta de remoção da senha global | Preservar a fonte em caso de falha e informar migração incompleta |
| Senha recusada | Solicitação de outra senha uma vez e indicação de suspensão após a segunda recusa | Não confundir recusa com falha de rede, TLS ou execução SQL |
| Exclusão da credencial | Confirmação separada, com identificação da conexão e aviso sobre outras cópias do projeto | Remover a conexão não autoriza apagar o segredo |

Os testes de tela atuais não validam os diálogos nativos do Keychain nem do Secret Service.
A [matriz do plano](../PLAN.md#fechamento-da-etapa-de-credenciais) separa implementação e validação pendentes.

## 6. Navegação e foco

O modo convencional é padrão. O modo Vim também atua na navegação da interface.
O mouse funciona nos dois modos.

Contextos que precisam de tratamento separado:

| Contexto | Comportamento |
|---|---|
| Navegador | Selecionar conexão ou objeto; abrir tabela por duplo clique |
| Abas principais | Alternar entre consoles e tabelas já abertos |
| Editor | Editar SQL; `Tab` aceita sugestões; `Enter` quebra a linha; executar somente por uma ação explícita |
| Filtros | `Tab` completa; `Enter` aplica |
| Resultados | Selecionar e copiar dados; navegar somente nas linhas carregadas |
| Diálogo | Resolver a pergunta sem acionar comandos de execução ao fundo |

O autocomplete inicial oferece schemas, tabelas, views e colunas; funções e assinaturas ficam fora desse escopo.
Sem catálogo disponível, sugerir as palavras-chave do dialeto.
Os atalhos de foco, abas e execução estão definidos em [KEYMAP.md](KEYMAP.md).
Atalhos de navegação não capturam caracteres durante a inserção de texto.
Dados, nomes e erros externos não devem ser interpretados como sequências de controle do terminal.

## 7. Retorno ao console

Reabrir um console recupera seu texto, cursor e rolagem.
Não recuperar dados das grades nem executar SQL automaticamente.
Se o console já estiver aberto, apenas selecionar a aba existente.
Se estiver em uso por outra instância, bloquear sua abertura nesta instância.

## 8. Esboços complementares

### Transação manual aberta

A aba **Mensagens** mostra avisos, erros e comandos sem linhas.

```text
┌─ console-1 ──────────────────────────────────────────────┐
│ Schema: [public]                                         │
│ [Executar] [Script inteiro] [Parar script]               │
│ Modo: [Manual] · Transação aberta · [Commit] [Rollback]  │
├──────────────────────────────────────────────────────────┤
│ 1 ✓ UPDATE users SET active = false WHERE id = 7;        │
├──────────────────────────────────────────────────────────┤
│ Mensagens                                                │
│ 1 linha alterada. Transação ainda não confirmada.        │
└──────────────────────────────────────────────────────────┘
```

### Espera no pool

**Parar script** cancela o aguardo sem enviar SQL ao banco.

```text
┌─ console-2 ──────────────────────────────────────────────┐
│ Aguardando conexão · Pool ocupado: 5/5                   │
│ O SQL ainda não foi enviado ao banco.                    │
│ [Parar script]                                           │
└──────────────────────────────────────────────────────────┘
```

### Cancelamento

O primeiro esboço mostra a espera de 5 segundos pela resposta da instrução.
O segundo aparece somente com transação aberta.

```text
┌─ Parar script ───────────────────────────────────────────┐
│ Cancelamento enviado. Aguardando a resposta do banco.    │
│ Tempo: 3 de 5 segundos                                   │
└──────────────────────────────────────────────────────────┘
```

```text
┌─ Fechar conexão ─────────────────────────────────────────┐
│ O banco não respondeu ao cancelamento.                   │
│ Fechar a conexão encerra a transação aberta.             │
│ As alterações não confirmadas serão perdidas.            │
│                                                          │
│ [Fechar conexão]  [Continuar aguardando]                 │
└──────────────────────────────────────────────────────────┘
```

### Orçamento esgotado

Nos consoles, o orçamento encerra a leitura como o teto de linhas.
Na visão de tabela, **Carregar mais** fica indisponível até existir espaço.

```text
┌─ Dados das grades ───────────────────────────────────────┐
│ Orçamento atingido: 256/256 MiB                          │
│ As linhas carregadas foram mantidas.                     │
│ Feche abas ou aumente o limite para carregar mais linhas. │
└──────────────────────────────────────────────────────────┘
```

### Cadastro de SQLite

Se o arquivo não existir, perguntar antes de criá-lo.

```text
┌─ Conexão ────────────────────────────────────────────────┐
│ Tipo:         [SQLite]                                   │
│ Nome:         [local]                                    │
│ Escopo:       [Projeto]  (pode alterar para Global)      │
│ Arquivo:      [data/app.db]                              │
│ foreign_keys: [Padrão do SQLite]                         │
│ Bloqueio:     prazo [5] segundos                         │
│ Pool:         [5]                                        │
│                                                          │
│ [Testar conexão]  [Salvar]  [Cancelar]                   │
└──────────────────────────────────────────────────────────┘
```

### Entrada por URI

Uma senha na URI segue as regras de credenciais e não fica na configuração do projeto.

```text
┌─ Conexão ────────────────────────────────────────────────┐
│ Tipo:    [PostgreSQL]                                    │
│ Nome:    [app]                                           │
│ Escopo:  [Projeto]  (pode alterar para Global)           │
│ Entrada: [URI]                                           │
│ URI:     [postgres://app_user@db.example.test/app_db]    │
│                                                          │
│ [Testar conexão]  [Salvar]  [Cancelar]                   │
└──────────────────────────────────────────────────────────┘
```

### Edição de conexão conectada

Dados de acesso, TLS e limite do pool exigem desconexão.

```text
┌─ Editar conexão: app (local) ────────────────────────────┐
│ Esta conexão está conectada.                             │
│ Desconecte para alterar acesso, TLS ou limite do pool.   │
│                                                          │
│ Nome: [app]                                              │
│                                                          │
│ [Desconectar]  [Salvar nome]  [Cancelar]                 │
└──────────────────────────────────────────────────────────┘
```

### Confirmação de destino

Esta pergunta aparece antes da primeira conexão a um destino nesta máquina, conforme a seção 3 do plano.
Mostrar destino, usuário e TLS; iniciar com foco em **Cancelar**.
A confirmação já existe no código. Um caminho novo pode mostrar **Confirm project path**.
Cancelar impede o envio da operação que espera essas credenciais.

```text
┌─ Confirmar destino ──────────────────────────────────────┐
│ Host:    db.example.test                                 │
│ Porta:   5432                                            │
│ Usuário: app_user                                        │
│ TLS:     Verificar certificado                           │
│                                                          │
│ [Confirmar]  [Cancelar]                                  │
└──────────────────────────────────────────────────────────┘
```

### Solicitação de senha

Iniciar com uso somente na sessão, com a opção de salvar desmarcada.
O texto da opção muda conforme o cofre.
Sem cofre, a opção informa que a gravação global usa **texto simples** e exige autorização.
Cofre bloqueado ou com erro não permite essa alternativa.

```text
┌─ Senha: app (local) ─────────────────────────────────────┐
│ Usuário: app_user                                        │
│ Senha:   [••••••••]                                      │
│                                                          │
│ [ ] Salvar no cofre do sistema                           │
│                                                          │
│ [Conectar]  [Cancelar]                                   │
└──────────────────────────────────────────────────────────┘
```

### Scripts sem conexão

A execução fica bloqueada até a escolha e a confirmação de uma conexão.

```text
┌─ Scripts sem conexão ────────────────────────────────────┐
│ investigar-pedidos       ontem, 16:20                    │
│ relatorio-mensal         12/10, 09:15                    │
│                                                          │
│ [Abrir]  [Associar a uma conexão]                        │
└──────────────────────────────────────────────────────────┘
```

### Configurações

**Definido, ainda não implementado:** abrir com `Ctrl+W ,` na primeira versão.
O projeto grava somente os valores que declara.
Oferecer escopo global ou do projeto e os cinco campos abaixo.
Mostrar os valores herdados do global e permitir remover uma definição do projeto para usar o global.
**Salvar** grava as alterações. **Cancelar** não altera nada.
Manter `config.json` para opções avançadas, como o `keymap`.
Ao mudar o modo de teclado, preservar texto, cursor e rolagem.
Se a mudança apagar seleção ou histórico, avisar antes, inclusive na recarga da configuração.

```text
┌─ Configurações ──────────────────────────────────────────┐
│ Escopo:             [Projeto]  (pode alterar para Global) │
│ Teclado:            [Convencional]                       │
│ Tamanho da página:  [100]                                │
│ Teto de linhas:     [1000]                               │
│ Orçamento de dados: [256] MiB                            │
│                                                          │
│ [Salvar]  [Cancelar]                                     │
└──────────────────────────────────────────────────────────┘
```

### Fechamento com transação aberta

O foco inicial fica em **Cancelar fechamento**, a opção sem efeito no banco.
Não existe commit como resposta padrão.

```text
┌─ Fechar console-1 ───────────────────────────────────────┐
│ Este console tem uma transação aberta.                   │
│                                                          │
│ [Commit e fechar]  [Rollback e fechar]                   │
│ [Cancelar fechamento]                                    │
└──────────────────────────────────────────────────────────┘
```

### Janela de detalhe

`Enter` na grade abre o valor completo. A janela é somente leitura.

```text
┌─ users.bio · linha 2 ────────────────────────────────────┐
│ Texto completo do valor selecionado, com quebra de       │
│ linha automática e rolagem.                              │
│                                                          │
│ 1.240 caracteres · texto                                 │
│ [Copiar]  [Fechar]                                       │
└──────────────────────────────────────────────────────────┘
```

### Tela de ajuda

A tela mostra os atalhos do modo ativo e do painel em foco.

```text
┌─ Ajuda · Convencional · Editor ──────────────────────────┐
│ Ctrl+Enter ou F5   Executar                              │
│ Shift+Enter        Script inteiro                        │
│ Ctrl+W x           Parar script                          │
│ Ctrl+Space         Sugestões                             │
│ Ctrl+F             Buscar e substituir                   │
│ Ctrl+W h/j/k/l     Mudar de painel                       │
│                                                          │
│ Esc  Fechar                                              │
└──────────────────────────────────────────────────────────┘
```

### Terminal pequeno

A aplicação preserva o estado e volta a desenhar quando o tamanho for suficiente.

```text
┌─ SQLN ───────────────────────────────────────────────────┐
│ Terminal pequeno: 52 x 14                                │
│ Tamanho mínimo: 80 x 24                                  │
│ Execuções e transações continuam ativas.                 │
└──────────────────────────────────────────────────────────┘
```

### Alterações externas em lote

Cada linha exige uma escolha. O salvamento dos arquivos listados fica pausado.

```text
┌─ Scripts alterados fora do SQLN ─────────────────────────┐
│ investigar-pedidos       [Recarregar] [Sobrescrever]     │
│ relatorio-mensal         [Recarregar] [Sobrescrever]     │
│ console-1 (removido)     [Recriar]    [Fechar]           │
│                                                          │
│ [Aplicar]  [Decidir depois]                              │
└──────────────────────────────────────────────────────────┘
```

### Busca e substituição no editor

`Ctrl+F` abre a barra. `Tab` passa para o campo de substituição.
As teclas da barra estão em [KEYMAP.md](KEYMAP.md).

```text
┌─ console-1 ──────────────────────────────────────────────┐
│ Buscar:     [users            ]  3 de 12                 │
│ Substituir: [people           ]  [Substituir] [Todos]    │
├──────────────────────────────────────────────────────────┤
│ 2 SELECT id FROM users;                                  │
│ 3 SELECT name FROM users;                                │
└──────────────────────────────────────────────────────────┘
```
