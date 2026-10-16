// What the Home window has selected, for the command palette: the selected task on Tasks and
// the selected / open items on the Inbox and Item detail. Provided by `HomeApp`.
import { createContext, useContext, useEffect, useMemo, useState, type ReactElement, type ReactNode } from 'react';

export interface HomeSelection { taskId?: string; itemIds: string[] }

interface SelectionState { selection: HomeSelection; setSelection: (s: HomeSelection) => void }

const EMPTY: HomeSelection = { itemIds: [] };
const SelectionContext = createContext<SelectionState>({ selection: EMPTY, setSelection: () => {} });

export function SelectionProvider({ children }: { children: ReactNode }): ReactElement {
  const [selection, setSelection] = useState<HomeSelection>(EMPTY);
  const value = useMemo(() => ({ selection, setSelection }), [selection]);
  return <SelectionContext.Provider value={value}>{children}</SelectionContext.Provider>;
}

export function useHomeSelection(): HomeSelection {
  return useContext(SelectionContext).selection;
}

/** Publishes the page's selection while mounted; clears it on unmount. */
export function useSetSelection(taskId: string | undefined, itemIds: readonly string[]): void {
  const { setSelection } = useContext(SelectionContext);
  const key = itemIds.join('\n');
  useEffect(() => {
    setSelection({ taskId, itemIds: key ? key.split('\n') : [] });
  }, [setSelection, taskId, key]);
  useEffect(() => () => setSelection(EMPTY), [setSelection]);
}
