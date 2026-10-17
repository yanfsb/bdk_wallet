| Sigla | Nome           | Significado                                             |
| ----- | -------------- | ------------------------------------------------------- |
| LR    | last_revealed  | last_revealed, maior índice logicamente revelado        |
| R     | Revealed count | quantidade de índices revelados logicamente             |
| U     | Used count     | quantidade de índices conhecidos como usados            |
| L     | Lookahead      | quantidade de SPKs futuros materializados               |
| D     | Distribution   | distribuição dos índices usados                         |
| SG    | Stop gap       | quantidade de índices vazios usada pelo modelo stop-gap |

Os números da seção seguinte são um exemplo didático pequeno (`LR = 30`), usado para explicar a construção das fixtures. A matriz do benchmark solicita `LR = 100_000` e as densidades nominais de `1%`, `5%`, `10%`, `25%`, `50%`, `75%` e `100%`, com `L = 20` e `SG = 10`. O perfil `gap_unconstrained` inclui os sete pontos; no perfil `gap_bounded`, `1%` e `5%` são inviáveis e registrados como não aplicáveis.

## 1. Exemplo didático: preparação do cenário

Usamos:

```text
  LR = 30
  R  = 31
  densidade nominal = 10%
  U  = ceil(31 × 10 / 100) = 4
  L  = 20
  SG = 10
```

fixture.rs produz duas distribuições:

```text
  bounded:       [0, 10, 20, 30]
  unconstrained: [0,  1,  2, 30]
```

Depois, history.rs deriva os quatro scripts usados e prepara:

```text
  script → output sintético
```

Tudo isso fica fora do timer.

## 2. Caminho metadata

A metadata já informa quais índices consultar.

A operação:

1.  Cria uma wallet nova.
2.  Restaura o frontier lógico 30.
3.  Materializa os índices usados, sem preencher os gaps.
4.  Consulta os quatro scripts conhecidos.
5.  Aplica os quatro outputs encontrados.

Na distribuição bounded, os SPKs materializados são:

```text
  lookahead inicial: 0..19
  usados:            0, 10, 20, 30

  união:             0..19, 20, 30
  total:             22
```

Portanto:

```text
  frontier lógico = 30
  scripts consultados = 4
  SPKs materializados = 22
```

Ter frontier 30 não exige materializar todos os índices até 30.

## 3. Caminho stop-gap

Esse caminho não usa o frontier da fixture para decidir onde parar. Ele consulta scripts
sequencialmente.

### Distribuição bounded

```text
  0 usado
  1..9 vazios
  10 usado → zera contador
  11..19 vazios
  20 usado → zera contador
  21..29 vazios
  30 usado → zera contador
  31..40 vazios → atingiu SG=10, para
```

Resultado:

```text
  scripts examinados = 41
  último índice ativo = 30
  recuperação completa
```

Ao aplicar a resposta, a wallet revela até 30 e mantém lookahead:

```text
  SPKs materializados = 31 + 20 = 51
```

### Distribuição unconstrained

```text
  0 usado
  1 usado
  2 usado
  3..12 vazios → atingiu SG=10, para
```

O uso em 30 não foi encontrado:

```text
  scripts examinados = 13
  último índice ativo = 2
  recuperação incompleta
  SPKs materializados = 3 + 20 = 23
```

## 4. O que a validação compara

| Perfil / estratégia      | Usados encontrados | Scripts examinados | Frontier | SPKs materializados | Completo |
| ------------------------ | ------------------ | ------------------ | -------- | ------------------- | -------- |
| Bounded / metadata       | 4                  | 4                  | 30       | 22                  | sim      |
| Bounded / stop-gap       | 4                  | 41                 | 30       | 51                  | sim      |
| Unconstrained / metadata | 4                  | 4                  | 30       | 21                  | sim      |
| Unconstrained / stop-gap | 3                  | 13                 | 2        | 23                  | não      |

O tempo menor do último caso não representa uma recuperação equivalente: ele encontrou menos estado.

Também não podemos chamar essas cardinalidades de “número de derivações criptográficas”. São métricas
distintas.

## 5. O que acontece em cada iteração

O histórico e o descriptor são compartilhados, mas a wallet nunca é reutilizada entre iterações:

```text
  Preparação, uma vez por cenário
      │
      ├── validar os dois caminhos fora do timer
      │
      └── Criterion repete:
            iniciar timer
            criar wallet nova
            executar recuperação
            retornar RecoveryRun
            encerrar timer
            descartar wallet
```

Isso evita medir uma segunda recuperação sobre uma wallet já preenchida.

Em restore.rs, a fronteira é diferente:

```text
  criar indexer fora do timer
  iniciar timer
  restaurar indexer
  encerrar timer
  descartar resultado
```

## 6. Por que separar operação e inspeção?

- recover*with*\*() executa a recuperação.
- inspect_run() observa o resultado.
- validate_case() verifica se ele está correto.
- metrics_row() transforma essa inspeção em CSV.

Se colocássemos inspeção, ordenação dos índices ou escrita do relatório dentro da operação medida,
estaríamos cronometrando também o nosso instrumento de análise.

Essa é a lógica central da estrutura: preparar → validar → medir a operação isolada → registrar.
