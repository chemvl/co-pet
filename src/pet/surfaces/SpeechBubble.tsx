// Pet speaks: one line in the pet's own voice; display only, no pointer, no focus; PetApp announces it.
import type { ReactElement } from 'react';

export function SpeechBubble({ text }: { text: string }): ReactElement {
  return (
    <div className="pt-speech" aria-hidden="true">
      {text}
    </div>
  );
}
