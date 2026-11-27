// The diagram's view: the room a fit leaves round the parts, and when a change
// of the diagram's size fits it again.

/**
 * The padding of every fit: room under the lowest parts for their names,
 * which hang below the parts at 11 px on screen whatever the zoom
 * (.ss-node-label), and above the highest ones for the project name and
 * toolbar floating over the diagram (`overlayHeight`, as tall as they are
 * drawn: the toolbar wraps in a narrow panel) and the run's value shown
 * over a part.
 */
export function fitPadding(overlayHeight: number) {
  return { x: 0.15, top: `${Math.max(48, Math.ceil(overlayHeight) + 24)}px`, bottom: "32px" } as const;
}

/**
 * Whether the diagram fits itself again after its size changed. A view the
 * last automatic fit left (`lastAutoView`) follows the diagram both ways, as
 * when the user opens or closes the bottom panel (GUI-03); a view the user
 * set is fitted again only when the change cut off a model that was entirely
 * in view (`cut`). A run opens no panel, so it never changes the diagram's
 * size or zoom.
 */
export function refitAfterResize(cut: boolean, lastAutoView: boolean): boolean {
  return cut || lastAutoView;
}
