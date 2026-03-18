let globalZIndex = 1000;
export function nextZIndex() { return ++globalZIndex; }
export function currentZIndex() { return globalZIndex; }
