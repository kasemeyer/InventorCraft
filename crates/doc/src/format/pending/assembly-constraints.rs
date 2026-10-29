//! An assembly saves its constraints (`assembly.constraints`): type, solution, picks by face, edge
//! and origin names, offset and limits with their expressions, and the solved values. Older designs
//! have none and read as before; nothing is rewritten. The last release refuses a design saved with
//! this (the newer format number).
