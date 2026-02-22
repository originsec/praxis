import dagre from '@dagrejs/dagre';
import type { ChainElement, ChainConnection } from '../api/types';

//
// Node sizes for different element types.
//
const NODE_SIZES: Record<string, { width: number; height: number }> = {
  Trigger: { width: 180, height: 60 },
  Operation: { width: 280, height: 180 },
  Transform: { width: 280, height: 140 },
  GenericPrompt: { width: 280, height: 120 },
  Memory: { width: 220, height: 70 },
  Loop: { width: 200, height: 60 },
  Tool: { width: 240, height: 80 },
  Payload: { width: 260, height: 100 },
  Termination: { width: 180, height: 60 },
};

/**
 * Get the element type from a chain element
 */
function getElementType(element: ChainElement): string {
  return element.element_type;
}

/**
 * Get the element ID from a chain element
 */
function getElementId(element: ChainElement): string {
  return element.id;
}

/**
 * Get the size for an element type
 */
export function getNodeSize(elementType: string): { width: number; height: number } {
  return NODE_SIZES[elementType] || { width: 180, height: 70 };
}

/**
 * Compute layout positions for chain elements using Dagre
 * Returns a map of element ID to position (top-left corner)
 */
export function computeLayout(
  elements: ChainElement[],
  connections: ChainConnection[]
): Map<string, { x: number; y: number }> {
  const g = new dagre.graphlib.Graph();

  //
  // Configure the graph.
  //
  g.setGraph({
    //
    // Left to right layout.
    //
    rankdir: 'LR',
    //
    // Horizontal separation between nodes at same rank.
    //
    nodesep: 60,
    //
    // Separation between ranks (columns).
    //
    ranksep: 120,
    marginx: 20,
    marginy: 20,
  });

  //
  // Set default edge label.
  //
  g.setDefaultEdgeLabel(() => ({}));

  //
  // Add nodes with their sizes.
  //
  for (const element of elements) {
    const id = getElementId(element);
    const type = getElementType(element);
    const size = getNodeSize(type);
    g.setNode(id, { width: size.width, height: size.height });
  }

  //
  // Add edges.
  //
  for (const conn of connections) {
    g.setEdge(conn.from_element, conn.to_element);
  }

  //
  // Run the layout algorithm.
  //
  dagre.layout(g);

  //
  // Extract positions (Dagre returns center points, convert to top-left).
  //
  const positions = new Map<string, { x: number; y: number }>();
  for (const id of g.nodes()) {
    const node = g.node(id);
    if (node) {
      positions.set(id, {
        x: node.x - node.width / 2,
        y: node.y - node.height / 2,
      });
    }
  }

  return positions;
}

/**
 * Compute layout and return ReactFlow-compatible node objects
 */
export function computeReactFlowLayout(
  elements: ChainElement[],
  connections: ChainConnection[]
): { id: string; position: { x: number; y: number }; data: ChainElement; type: string }[] {
  const positions = computeLayout(elements, connections);

  return elements.map(element => {
    const id = getElementId(element);
    const position = positions.get(id) || { x: 0, y: 0 };
    const type = getElementType(element);

    return {
      id,
      position,
      data: element,
      //
      // e.g., 'triggerNode', 'operationNode'.
      //
      type: type.toLowerCase() + 'Node',
    };
  });
}
