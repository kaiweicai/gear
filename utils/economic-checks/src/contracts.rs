// This file is part of Gear.

// Copyright (C) 2021 Gear Technologies Inc.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![allow(unused)]

pub const COMPOSE_WASM_BINARY: &[u8] = include_bytes!("code/demo_compose.opt.wasm");
pub const GENERAL_WASM_BINARY: &[u8] = include_bytes!("code/demo_contract_template.opt.wasm");
pub const MUL_CONST_WASM_BINARY: &[u8] = include_bytes!("code/demo_mul_by_const.opt.wasm");
pub const NCOMPOSE_WASM_BINARY: &[u8] = include_bytes!("code/demo_ncompose.opt.wasm");
pub const UNCHECKED_MUL_WASM_BINARY: &[u8] = include_bytes!("code/demo_unchecked_mul.opt.wasm");
