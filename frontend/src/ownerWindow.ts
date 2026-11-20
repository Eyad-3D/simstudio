import { createContext, useContext } from "react";

/** The window a component is drawn in: the main window, or a part's
 *  parameter window (components/ParamWindows.tsx), whose content is
 *  rendered from the main window. Key and pointer listeners that a
 *  component puts on "its" window go on this one, or they would hear the
 *  other window's keys. */
export const OwnerWindow = createContext<Window>(window);

export const useOwnerWindow = () => useContext(OwnerWindow);
