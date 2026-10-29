//! Reference parameters (R122): a driven dimension is named from the same `dN` sequence as a
//! driving one, its value is what it measured at its last solve, and it cannot be changed. An
//! expression of a driving dimension that names a driven dimension of the same sketch is solved
//! as an equality: the driving dimension follows what the driven one measures, in the same
//! solve (so `d1 = d2` with d2 measured from the other side centres a feature).

use std::collections::BTreeMap;

use solidcraft_sketch::{Constraint, DimEquation, DimEquations, Sketch};

use crate::expr::{self, Kind, ParamDef, Value};
use crate::{DocError, Document, FeatureKind, Param, Result};

/// A driven dimension's parameter expression: what it measures, rounded to a nanometre (or a
/// nano-degree), with its unit.
fn measured_expr(c: &Constraint) -> Option<(String, &'static str)> {
    let v = c.kind.value().filter(|v| v.is_finite())?;
    Some(if c.kind.is_angle() {
        (format!("{} deg", (v.to_degrees() * 1e9).round() / 1e9), "deg")
    } else {
        (format!("{} mm", (v * 1e9).round() / 1e9), "mm")
    })
}

/// A dimension that can carry a parameter.
fn is_dimension(c: &Constraint) -> bool {
    c.kind.is_dimension() && c.kind.value().is_some()
}

impl Document {
    /// Is `name` a reference parameter (a driven dimension's)?
    pub fn is_reference_param(&self, name: &str) -> bool {
        self.param(name).is_some_and(|p| p.reference)
    }

    /// Every driven dimension has a reference parameter holding what it measures, and no
    /// driving dimension's parameter is marked as one.
    pub fn reference_params_in_sync(&self) -> bool {
        self.features.iter().all(|f| match &f.kind {
            FeatureKind::Sketch { sketch, .. } => sketch.constraints.iter().filter(|c| is_dimension(c)).all(|c| match (&c.param, c.driven) {
                (None, driven) => !driven,
                (Some(n), true) => {
                    self.param(n).is_some_and(|p| p.reference && p.model && measured_expr(c).is_none_or(|(e, unit)| p.expr == e && p.unit == unit))
                }
                (Some(n), false) => !self.is_reference_param(n),
            }),
            _ => true,
        })
    }

    /// Name every driven dimension (in feature order, then constraint order, from the `dN`
    /// sequence; a dimension keeps the name it has), write what each measures into its
    /// reference parameter, and turn a reference parameter whose dimension became driving back
    /// into a model parameter (its last measured value is then its driving value). Changes
    /// nothing when they already agree.
    pub fn sync_reference_params(&mut self) {
        if self.reference_params_in_sync() {
            return;
        }
        for fi in 0..self.features.len() {
            let count = match self.features.get(fi).map(|f| &f.kind) {
                Some(FeatureKind::Sketch { sketch, .. }) => sketch.constraints.len(),
                _ => continue,
            };
            for ci in 0..count {
                let Some(FeatureKind::Sketch { sketch, .. }) = self.features.get(fi).map(|f| &f.kind) else { break };
                let Some(c) = sketch.constraints.get(ci).filter(|c| is_dimension(c)) else { continue };
                let (param, driven, measured) = (c.param.clone(), c.driven, measured_expr(c));
                if !driven {
                    if let Some(p) = param.as_deref().and_then(|n| self.params.iter_mut().find(|p| p.name == n)) {
                        p.reference = false;
                    }
                    continue;
                }
                let Some((e, unit)) = measured else { continue };
                let name = param.unwrap_or_else(|| self.next_d_name());
                match self.params.iter_mut().find(|p| p.name == name) {
                    Some(p) => {
                        p.expr = e;
                        p.unit = unit.into();
                        p.model = true;
                        p.reference = true;
                    }
                    None => self.params.push(Param {
                        name: name.clone(),
                        expr: e,
                        unit: unit.into(),
                        comment: String::new(),
                        model: true,
                        reference: true,
                    }),
                }
                if let Some(FeatureKind::Sketch { sketch, .. }) = self.features.get_mut(fi).map(|f| &mut f.kind)
                    && let Some(c) = sketch.constraints.get_mut(ci)
                {
                    c.param = Some(name);
                }
            }
        }
    }

    /// Refuse to change a reference parameter's expression.
    pub(crate) fn refuse_reference_change(&self, name: &str) -> Result<()> {
        if self.is_reference_param(name) {
            return Err(DocError::Invalid(format!(
                "`{name}` is a reference parameter: it reports what its driven dimension measures and cannot be changed (make the dimension driving to set it)"
            )));
        }
        Ok(())
    }

    /// The equations of a sketch's driving dimensions whose expressions name its driven ones
    /// (see the module): each expression, made affine around the driven dimensions' last
    /// measured values (exact for `d2`, `d2 + 1`, `2 * d2`, …).
    pub(crate) fn dimension_equations(&self, vals: &BTreeMap<String, Value>, sk: &Sketch) -> DimEquations {
        // Reference names of this sketch → their driven constraint ids.
        let driven: BTreeMap<&str, &str> = sk
            .constraints
            .iter()
            .filter(|c| c.driven)
            .filter_map(|c| c.param.as_deref().filter(|n| self.is_reference_param(n)).map(|n| (n, c.id.as_str())))
            .collect();
        if driven.is_empty() {
            return DimEquations::default();
        }
        let len = crate::params::doc_len(&self.units);
        let mut out = Vec::new();
        for c in sk.constraints.iter().filter(|c| !c.driven) {
            let Some(p) = c.param.as_deref().and_then(|n| self.param(n)) else { continue };
            let refs: Vec<String> = expr::references(&p.expr).into_iter().filter(|r| driven.contains_key(r.as_str())).collect();
            if refs.is_empty() {
                continue;
            }
            let kind = if c.kind.is_angle() { Kind::Angle } else { Kind::Length };
            let def = ParamDef { name: p.name.clone(), expr: p.expr.clone(), unit: p.unit.clone() };
            // The expression's value with one reference moved by `shift`.
            let at = |shift: Option<(&str, f64)>| -> Option<f64> {
                let lookup = |n: &str| -> std::result::Result<Value, DocError> {
                    let v = vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")))?;
                    Ok(match shift {
                        Some((s, d)) if s == n => Value { v: v.v + d, ..v },
                        _ => v,
                    })
                };
                expr::eval_def(&def, &lookup, len).ok()?.to_kind(kind).ok()
            };
            let Some(mut constant) = at(None) else { continue };
            let mut terms = Vec::new();
            for r in &refs {
                let (Some(v0), Some(id)) = (vals.get(r).map(|v| v.v), driven.get(r.as_str())) else { continue };
                let h = 1e-6 * v0.abs().max(1.0);
                let (Some(up), Some(down)) = (at(Some((r, h))), at(Some((r, -h)))) else { continue };
                let k = (up - down) / (2.0 * h);
                constant -= k * v0;
                terms.push((id.to_string(), k));
            }
            if !terms.is_empty() {
                out.push(DimEquation { driving: c.id.clone(), constant, terms });
            }
        }
        DimEquations(out)
    }
}
