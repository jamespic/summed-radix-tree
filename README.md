Summed Radix Tree
=================

This package contains a specialised data structure. A persistent summed radix tree.

It's essentially a immutable mapping from ints to ints, where you can get the total efficiently,
you can add items or merge mappings efficiently, and all updates create a copy, leaving the
original untouched.

Why would you want this? The most obvious use case is walking a directed acyclic graph, adding up
item weights for subtrees.

Use example:

```
from summed_radix_tree import SummedRadixTree
tree_a = SummedRadixTree({1: 10, 2: 20, 3: 30})
tree_b = SummedRadixTree().add(4, 40)
assert (tree_a | tree_b).total == 100
assert tree_a.total == 60 # Original trees are unaffected
assert tree_b.total == 40
assert (tree_a | tree_b).total == 100

tree_c = SummedRadixTree({2: 20, 4: 40}) # Overlaps other trees
assert (tree_a | tree_b | tree_c).total == 100 # Total is still right
```