# 0393. A bet is settled only by a named test or a measured number, and a part settled is marked `partly`

**Status:** Accepted. Design decision, made by the design architect with the lead. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the [register of hypotheses](../hypotheses/README.md): one new status, the rule for leaving `open`, nine statuses, and a result file for H-11. Nothing in the product.

## Context

On 2026-11-24 the register held 53 bets, of which 51 were `open`, after about 120 packages. The notes under the register were kept well: most open bets carry dated notes that say what was run and what was not. What the status column did not show is the difference between a bet nothing has touched and a bet of which half is proven by a test in CI. A reader of the column alone learns nothing from "51 open".

The opposite fault is worse. A bet closed because the code reads as if it holds is a guess given the standing of a result.

## Decision

1. **A status other than `open` needs a named test and its result, or a measured number.** A reading of the code, a search of the sources, and a design that went one way are notes, not results.
2. **`partly` is a status.** It is an open bet of which a named test or a number has settled a stated part. Its result file says which part, by which test or record, and what is missing. A status cell holds one word and nothing else, because `tools/docs_audit.py` reads it; the first form of this sweep wrote sentences in the cells, which the audit's pattern did not match, so its check passed over those rows without a word. Until the audit accepts `partly`, the cell of such a bet stays `open` and its result file's status line says `partly`. A bet that is `partly` is still open: nothing may be built on the missing part as if it held.
3. **`supported`, `refuted`, and `changed` need a result file,** as before.
4. **The sweep of 2026-11-24** applied this to every bet. H-11 is `changed`, with its result file written from the numbers of [decision 0082](0082-the-decision-core-is-the-policy-crates-own-evaluator.md). H-04, H-06, H-16, H-20, H-28, H-32, H-39, and H-40 are `partly`, each with a result file. H-26's cell is made the one word `supported`. The register's note of the sweep names the bets that lean one way with no test and stay `open`.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Leave the column as it is and let the notes carry it | The column is what a slice's plan reads first, and it said the same of a bet half proven and a bet untouched |
| Close each bet whose notes lean clearly one way | Several lean on a reading of the code. Forty open and true is worth more than forty closed by inference |
| A percentage for each bet | A guess stated as a number |

## Revisit if

A bet marked `partly` is found to have been built on in its missing part.
