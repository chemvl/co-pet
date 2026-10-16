// Quick note panel (plan Step 5.6): one line, Enter or Save captures it, Esc closes.
import { useState } from 'react';
import { Button } from '../../ds';
import { useT } from '../../shared/i18n';

export function QuickNotePanel(p: { onSave(text: string): void; onClose(): void }) {
  const t = useT();
  const [text, setText] = useState('');
  const save = () => {
    const note = text.trim();
    if (note) p.onSave(note);
  };
  return (
    <form
      className="pt-quicknote"
      aria-label={t('pet.quickNote.label')}
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <input
        className="pt-quicknote__input body"
        autoFocus
        value={text}
        placeholder={t('pet.quickNote.placeholder')}
        aria-label={t('pet.quickNote.label')}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.preventDefault();
            p.onClose();
          }
        }}
      />
      <Button type="submit" variant="primary" size="sm" disabled={!text.trim()}>
        {t('common.save')}
      </Button>
    </form>
  );
}
