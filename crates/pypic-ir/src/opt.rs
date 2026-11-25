//! Optimizaciones de la IR (#6, docs/ir-optimizaciones.md). Conservadoras, por funcion, y con la misma semantica que el interprete (usan `eval`).
//!
//! * `propagate`: dentro de cada bloque, propaga constantes y copias de temporales y locales a sus lecturas y pliega las operaciones que
//!   quedan con operandos constantes (`Binary`, `Unary`, `Compare`, `Cast`). **Nunca** toca globales (una interrupcion puede cambiarlas) ni los
//!   intrinsecos de `pypic.bits` (`BitSet` y `BitClear` modifican su variable en su sitio).
//! * `simplify_cfg`: ramas con condicion constante o con los dos destinos iguales pasan a saltos, se saltan los bloques vacios (`bb2: jump bb3`),
//!   se funde un bloque con su unico sucesor y se quitan los bloques inalcanzables.
//! * `eliminate_dead`: quita las instrucciones puras cuyo resultado (temporal o local) nunca se lee. Las que tienen efectos (llamadas, esperas,
//!   perifericos, pines, escrituras a globales o arreglos) no se tocan.
//! * `compact_temps`: renumera los temporales que quedan, para que los muertos no gasten RAM.
//!
//! Las funciones `@asm_function` no se tocan. Lo que **no** hace: reordenar instrucciones, eliminar subexpresiones comunes, propagar entre bloques,
//! simplificar algebraicamente (`x + 0`), ni quitar lecturas de arreglos (el interprete anota los accesos fuera de rango).

use std::collections::{HashMap, HashSet};

use crate::eval;
use crate::ir::{Function, Inst, Module, Operand, Place, Terminator, Type};

/// Lo que hizo cada pasada, para informar y para las pruebas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub propagated: usize,
    pub folded: usize,
    pub branches_simplified: usize,
    pub blocks_removed: usize,
    pub insts_removed: usize,
    pub temps_removed: usize,
}

impl Stats {
    pub fn total(&self) -> usize {
        self.propagated
            + self.folded
            + self.branches_simplified
            + self.blocks_removed
            + self.insts_removed
            + self.temps_removed
    }
}

/// Todas las pasadas hasta que ninguna cambia nada, y la compactacion de temporales.
pub fn optimize(m: &mut Module) -> Stats {
    let mut st = Stats::default();
    for _ in 0..16 {
        let before = st;
        propagate(m, &mut st);
        simplify_cfg(m, &mut st);
        eliminate_dead(m, &mut st);
        if st == before {
            break;
        }
    }
    compact_temps(m, &mut st);
    st
}

fn global_types(m: &Module) -> Vec<Type> {
    m.globals.iter().map(|g| g.ty).collect()
}

fn ty_of(f: &Function, gty: &[Type], p: Place) -> Type {
    match p {
        Place::Temp(i) => f.temps[i as usize],
        Place::Local(i) => f.locals[i as usize].ty,
        Place::Global(i) => gty[i as usize],
    }
}

fn operand_ty(f: &Function, gty: &[Type], o: &Operand) -> Type {
    match o {
        Operand::Const { ty, .. } => *ty,
        Operand::Place(p) => ty_of(f, gty, *p),
    }
}

// ---- recorrer los operandos y destinos de una instruccion ----------------------------------------------------------------------------------

/// Todas las posiciones de operando de una instruccion (lecturas).
fn operands_mut(inst: &mut Inst) -> Vec<&mut Operand> {
    match inst {
        Inst::Copy { src, .. } | Inst::Unary { src, .. } | Inst::Cast { src, .. } => vec![src],
        Inst::Binary { lhs, rhs, .. } | Inst::Compare { lhs, rhs, .. } => vec![lhs, rhs],
        Inst::Call { args, .. } => args.iter_mut().collect(),
        Inst::PinSet { value, .. } => vec![value],
        Inst::Delay { amount, .. } => vec![amount],
        Inst::Timer { arg, .. } | Inst::Reg { arg, .. } | Inst::Uart { arg, .. } => {
            arg.iter_mut().collect()
        }
        Inst::Pwm { hi, dc, .. } => hi.iter_mut().chain(dc.iter_mut()).collect(),
        Inst::Bits { src, .. } => src.iter_mut().collect(),
        Inst::RomGet { index, .. } | Inst::ArrGet { index, .. } => vec![index],
        Inst::ArrSet { index, value, .. } => vec![index, value],
        Inst::PinCfg { .. } | Inst::PinToggle { .. } | Inst::PinGet { .. } | Inst::Adc { .. } => {
            vec![]
        }
    }
}

/// Los lugares que escribe la instruccion como destino (no incluye el `src` de `Bits`, que se modifica en su sitio).
fn dsts_mut(inst: &mut Inst) -> Vec<&mut Place> {
    match inst {
        Inst::Copy { dst, .. }
        | Inst::Unary { dst, .. }
        | Inst::Binary { dst, .. }
        | Inst::Compare { dst, .. }
        | Inst::Cast { dst, .. }
        | Inst::PinGet { dst, .. }
        | Inst::RomGet { dst, .. }
        | Inst::ArrGet { dst, .. } => vec![dst],
        Inst::Call { dst, .. }
        | Inst::Timer { dst, .. }
        | Inst::Reg { dst, .. }
        | Inst::Bits { dst, .. }
        | Inst::Adc { dst, .. }
        | Inst::Uart { dst, .. } => dst.iter_mut().collect(),
        Inst::PinCfg { .. }
        | Inst::PinSet { .. }
        | Inst::PinToggle { .. }
        | Inst::Delay { .. }
        | Inst::Pwm { .. }
        | Inst::ArrSet { .. } => vec![],
    }
}

/// Lugares que la instruccion escribe: sus destinos y, en `Bits`, la variable que modifica en su sitio.
fn written(inst: &Inst) -> Vec<Place> {
    let mut tmp = inst.clone();
    let mut out: Vec<Place> = dsts_mut(&mut tmp).into_iter().map(|p| *p).collect();
    if let Inst::Bits {
        src: Some(Operand::Place(p)),
        ..
    } = inst
    {
        out.push(*p);
    }
    out
}

/// Lugares que la instruccion lee.
fn read_places(inst: &Inst) -> Vec<Place> {
    let mut tmp = inst.clone();
    operands_mut(&mut tmp)
        .into_iter()
        .filter_map(|o| {
            if let Operand::Place(p) = o {
                Some(*p)
            } else {
                None
            }
        })
        .collect()
}

fn term_operands_mut(t: &mut Terminator) -> Vec<&mut Operand> {
    match t {
        Terminator::Branch { cond, .. } => vec![cond],
        Terminator::Return(Some(o)) => vec![o],
        _ => vec![],
    }
}

fn trackable(p: Place) -> bool {
    matches!(p, Place::Temp(_) | Place::Local(_))
}

// ---- propagacion y plegado -------------------------------------------------------------------------------------------------------------

pub fn propagate(m: &mut Module, st: &mut Stats) {
    let gty = global_types(m);
    for f in &mut m.functions {
        if f.asm.is_none() {
            propagate_function(f, &gty, st);
        }
    }
}

fn propagate_function(f: &mut Function, gty: &[Type], st: &mut Stats) {
    let mut blocks = std::mem::take(&mut f.blocks);
    for block in &mut blocks {
        let mut known: HashMap<Place, Operand> = HashMap::new();
        for inst in &mut block.insts {
            if !matches!(inst, Inst::Bits { .. }) {
                for o in operands_mut(inst) {
                    if let Operand::Place(p) = o {
                        if let Some(v) = known.get(p) {
                            *o = *v;
                            st.propagated += 1;
                        }
                    }
                }
            }
            fold(f, gty, inst, st);
            for p in written(inst) {
                known.remove(&p);
                known.retain(|_, v| *v != Operand::Place(p));
            }
            if let Inst::Copy { dst, src } = inst {
                let src_ok = match src {
                    Operand::Const { .. } => true,
                    Operand::Place(p) => trackable(*p),
                };
                if trackable(*dst)
                    && src_ok
                    && operand_ty(f, gty, src) == ty_of(f, gty, *dst)
                    && *src != Operand::Place(*dst)
                {
                    known.insert(*dst, *src);
                }
            }
        }
        for o in term_operands_mut(&mut block.term) {
            if let Operand::Place(p) = o {
                if let Some(v) = known.get(p) {
                    *o = *v;
                    st.propagated += 1;
                }
            }
        }
    }
    f.blocks = blocks;
}

/// Pliega una operacion con todos sus operandos constantes a una copia de la constante resultante.
fn fold(f: &Function, gty: &[Type], inst: &mut Inst, st: &mut Stats) {
    let konst = |o: &Operand| {
        if let Operand::Const { value, .. } = o {
            Some(*value)
        } else {
            None
        }
    };
    let folded = match inst {
        Inst::Binary { dst, op, lhs, rhs } => konst(lhs)
            .zip(konst(rhs))
            .map(|(a, b)| (*dst, eval::binary(*op, a, b))),
        Inst::Unary { dst, op, src } => konst(src).map(|v| (*dst, eval::unary(*op, v))),
        Inst::Compare { dst, op, lhs, rhs } => konst(lhs)
            .zip(konst(rhs))
            .map(|(a, b)| (*dst, eval::compare(*op, a, b) as i128)),
        Inst::Cast { dst, src } => konst(src).map(|v| (*dst, v)),
        _ => None,
    };
    if let Some((dst, v)) = folded {
        let ty = ty_of(f, gty, dst);
        *inst = Inst::Copy {
            dst,
            src: Operand::Const {
                value: eval::stored(ty, v),
                ty,
            },
        };
        st.folded += 1;
    }
}

// ---- grafo de bloques ------------------------------------------------------------------------------------------------------------------

pub fn simplify_cfg(m: &mut Module, st: &mut Stats) {
    for f in &mut m.functions {
        if f.asm.is_none() {
            simplify_function(f, st);
        }
    }
}

fn targets(t: &Terminator) -> Vec<usize> {
    match t {
        Terminator::Jump(b) => vec![*b],
        Terminator::Branch {
            then_bb, else_bb, ..
        } => vec![*then_bb, *else_bb],
        _ => vec![],
    }
}

fn simplify_function(f: &mut Function, st: &mut Stats) {
    loop {
        let mut changed = false;
        // ramas con condicion constante o con los dos destinos iguales
        for b in &mut f.blocks {
            if let Terminator::Branch {
                cond,
                then_bb,
                else_bb,
            } = &b.term
            {
                let to = match cond {
                    Operand::Const { value, .. } => {
                        Some(if *value != 0 { *then_bb } else { *else_bb })
                    }
                    _ if then_bb == else_bb => Some(*then_bb),
                    _ => None,
                };
                if let Some(t) = to {
                    b.term = Terminator::Jump(t);
                    st.branches_simplified += 1;
                    changed = true;
                }
            }
        }
        // saltar los bloques vacios que solo saltan a otro
        let empty_jump: Vec<Option<usize>> = f
            .blocks
            .iter()
            .map(|b| {
                if b.insts.is_empty() {
                    if let Terminator::Jump(t) = b.term {
                        Some(t)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        let forward = |mut b: usize| {
            for _ in 0..empty_jump.len() {
                match empty_jump[b] {
                    Some(n) if n != b => b = n,
                    _ => break,
                }
            }
            b
        };
        for b in &mut f.blocks {
            match &mut b.term {
                Terminator::Jump(t) => {
                    let n = forward(*t);
                    if n != *t {
                        *t = n;
                        changed = true;
                    }
                }
                Terminator::Branch {
                    then_bb, else_bb, ..
                } => {
                    for t in [then_bb, else_bb] {
                        let n = forward(*t);
                        if n != *t {
                            *t = n;
                            changed = true;
                        }
                    }
                }
                _ => {}
            }
        }
        // fundir un bloque con su unico sucesor
        let mut preds = vec![0usize; f.blocks.len()];
        for b in reachable(f)
            .iter()
            .enumerate()
            .filter(|(_, r)| **r)
            .map(|(i, _)| i)
        {
            for t in targets(&f.blocks[b].term) {
                preds[t] += 1;
            }
        }
        let reach = reachable(f);
        for b in 0..f.blocks.len() {
            if !reach[b] {
                continue;
            }
            while let Terminator::Jump(t) = f.blocks[b].term {
                if t == b || t == 0 || preds[t] != 1 || !reach[t] {
                    break;
                }
                let moved = std::mem::take(&mut f.blocks[t].insts);
                f.blocks[b].insts.extend(moved);
                f.blocks[b].term =
                    std::mem::replace(&mut f.blocks[t].term, Terminator::Unreachable);
                preds[t] = 0;
                changed = true;
            }
        }
        if remove_unreachable(f, st) {
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

fn reachable(f: &Function) -> Vec<bool> {
    let mut seen = vec![false; f.blocks.len()];
    let mut stack = vec![0usize];
    while let Some(b) = stack.pop() {
        if std::mem::replace(&mut seen[b], true) {
            continue;
        }
        stack.extend(targets(&f.blocks[b].term));
    }
    seen
}

fn remove_unreachable(f: &mut Function, st: &mut Stats) -> bool {
    let seen = reachable(f);
    if seen.iter().all(|s| *s) {
        return false;
    }
    let mut new_id = vec![usize::MAX; f.blocks.len()];
    let mut next = 0;
    for (i, s) in seen.iter().enumerate() {
        if *s {
            new_id[i] = next;
            next += 1;
        }
    }
    let old = std::mem::take(&mut f.blocks);
    st.blocks_removed += old.len() - next;
    f.blocks = old
        .into_iter()
        .enumerate()
        .filter(|(i, _)| seen[*i])
        .map(|(_, mut b)| {
            match &mut b.term {
                Terminator::Jump(t) => *t = new_id[*t],
                Terminator::Branch {
                    then_bb, else_bb, ..
                } => {
                    *then_bb = new_id[*then_bb];
                    *else_bb = new_id[*else_bb];
                }
                _ => {}
            }
            b
        })
        .collect();
    true
}

// ---- codigo muerto ---------------------------------------------------------------------------------------------------------------------

pub fn eliminate_dead(m: &mut Module, st: &mut Stats) {
    for f in &mut m.functions {
        if f.asm.is_none() {
            loop {
                let mut read: HashSet<Place> = HashSet::new();
                for b in &mut f.blocks {
                    for i in &b.insts {
                        read.extend(read_places(i));
                        // BitSet y BitClear modifican su variable: la leen y la escriben
                        if let Inst::Bits {
                            src: Some(Operand::Place(p)),
                            ..
                        } = i
                        {
                            read.insert(*p);
                        }
                    }
                    for o in term_operands_mut(&mut b.term) {
                        if let Operand::Place(p) = o {
                            read.insert(*p);
                        }
                    }
                }
                let mut removed = 0;
                for b in &mut f.blocks {
                    let before = b.insts.len();
                    b.insts.retain(|i| !is_dead(i, &read));
                    removed += before - b.insts.len();
                }
                st.insts_removed += removed;
                if removed == 0 {
                    break;
                }
            }
        }
    }
}

/// Una instruccion pura cuyo resultado (temporal o local) nadie lee. Las lecturas de arreglos no se quitan: el interprete anota sus accesos.
fn is_dead(i: &Inst, read: &HashSet<Place>) -> bool {
    match i {
        Inst::Copy { dst, .. }
        | Inst::Unary { dst, .. }
        | Inst::Binary { dst, .. }
        | Inst::Compare { dst, .. }
        | Inst::Cast { dst, .. }
        | Inst::RomGet { dst, .. } => trackable(*dst) && !read.contains(dst),
        _ => false,
    }
}

// ---- temporales -----------------------------------------------------------------------------------------------------------------------

pub fn compact_temps(m: &mut Module, st: &mut Stats) {
    for f in &mut m.functions {
        if f.asm.is_some() {
            continue;
        }
        let mut used: Vec<bool> = vec![false; f.temps.len()];
        for b in &mut f.blocks {
            for i in &mut b.insts {
                for o in operands_mut(i) {
                    if let Operand::Place(Place::Temp(t)) = o {
                        used[*t as usize] = true;
                    }
                }
                for p in dsts_mut(i) {
                    if let Place::Temp(t) = p {
                        used[*t as usize] = true;
                    }
                }
            }
            for o in term_operands_mut(&mut b.term) {
                if let Operand::Place(Place::Temp(t)) = o {
                    used[*t as usize] = true;
                }
            }
        }
        if used.iter().all(|u| *u) {
            continue;
        }
        let mut map = vec![u32::MAX; f.temps.len()];
        let mut kept = Vec::new();
        for (i, u) in used.iter().enumerate() {
            if *u {
                map[i] = kept.len() as u32;
                kept.push(f.temps[i]);
            }
        }
        st.temps_removed += f.temps.len() - kept.len();
        f.temps = kept;
        let renumber = |p: &mut Place| {
            if let Place::Temp(t) = p {
                *t = map[*t as usize];
            }
        };
        for b in &mut f.blocks {
            for i in &mut b.insts {
                for o in operands_mut(i) {
                    if let Operand::Place(p) = o {
                        renumber(p);
                    }
                }
                for p in dsts_mut(i) {
                    renumber(p);
                }
            }
            for o in term_operands_mut(&mut b.term) {
                if let Operand::Place(p) = o {
                    renumber(p);
                }
            }
        }
    }
}
