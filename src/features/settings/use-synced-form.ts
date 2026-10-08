import { useState } from 'react'

type Codec<S, F> = {
  toForm: (saved: S) => F
  // Null while the form is invalid.
  read: (form: F) => S | null
  same: (a: S, b: S) => boolean
}

/**
 * Form state for settings that other windows can change at any time.
 *
 * A form without edits follows the stored value. A form with edits keeps
 * them and reports `outdated` instead, so saving it cannot silently undo
 * the other window's change.
 */
export function useSyncedForm<S, F>(saved: S, codec: Codec<S, F>) {
  const [form, setForm] = useState(() => codec.toForm(saved))
  // The stored value the edits started from.
  const [base, setBase] = useState(saved)
  const [seen, setSeen] = useState(saved)
  const [outdated, setOutdated] = useState(false)
  const current = codec.read(form)
  const dirty = current === null || !codec.same(current, base)

  // Adjusting state while rendering avoids a frame that shows the old value.
  if (saved !== seen) {
    setSeen(saved)
    if (
      codec.same(saved, base) ||
      (current !== null && codec.same(saved, current))
    ) {
      setBase(saved)
      setOutdated(false)
    } else if (!dirty) {
      setForm(codec.toForm(saved))
      setBase(saved)
      setOutdated(false)
    } else {
      setOutdated(true)
    }
  }

  return {
    form,
    setForm,
    dirty,
    outdated,
    /** Drops the edits and starts again from the stored value. */
    reset() {
      setForm(codec.toForm(saved))
      setBase(saved)
      setOutdated(false)
    },
    /** Marks `next` as stored before the new value arrives through `saved`. */
    committed(next: S) {
      setBase(next)
      setOutdated(false)
    }
  }
}
