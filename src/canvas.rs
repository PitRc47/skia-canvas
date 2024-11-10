#![allow(non_snake_case)]
use std::cell::RefCell;
use neon::{prelude::*, types::buffer::TypedArray};

use crate::utils::*;
use crate::context::page::pages_arg;
use crate::gpu;

pub type BoxedCanvas = JsBox<RefCell<Canvas>>;
impl Finalize for Canvas {}

pub struct Canvas{
  pub width: f32,
  pub height: f32,
  engine: Option<gpu::RenderingEngine>,
}

impl Canvas{
  pub fn new() -> Self{
    Canvas{width:300.0, height:150.0, engine:None}
  }

  pub fn engine(&mut self) -> gpu::RenderingEngine{
    self.engine.get_or_insert_with(||
      gpu::RenderingEngine::default()
    ).clone()
  }
}

//
// -- Javascript Methods --------------------------------------------------------------------------
//

pub fn new(mut cx: FunctionContext) -> JsResult<BoxedCanvas> {
  let this = RefCell::new(Canvas::new());
  Ok(cx.boxed(this))
}

pub fn get_width(mut cx: FunctionContext) -> JsResult<JsNumber> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let width = this.borrow().width;
  Ok(cx.number(width as f64))
}

pub fn get_height(mut cx: FunctionContext) -> JsResult<JsNumber> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let height = this.borrow().height;
  Ok(cx.number(height as f64))
}

pub fn set_width(mut cx: FunctionContext) -> JsResult<JsUndefined> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let width = float_arg(&mut cx, 1, "size")?;
  this.borrow_mut().width = width;
  Ok(cx.undefined())
}

pub fn set_height(mut cx: FunctionContext) -> JsResult<JsUndefined> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let height = float_arg(&mut cx, 1, "size")?;
  this.borrow_mut().height = height;
  Ok(cx.undefined())
}

pub fn get_engine(mut cx: FunctionContext) -> JsResult<JsString> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let mut this = this.borrow_mut();
  Ok(cx.string(from_engine(this.engine())))
}

pub fn set_engine(mut cx: FunctionContext) -> JsResult<JsUndefined> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  if let Some(engine_name) = opt_string_arg(&mut cx, 1){
    if let Some(new_engine) = to_engine(&engine_name){
      if new_engine.selectable() {
        this.borrow_mut().engine = Some(new_engine)
      }
    }
  }

  Ok(cx.undefined())
}

pub fn get_engine_status(mut cx: FunctionContext) -> JsResult<JsString> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let mut this = this.borrow_mut();

  let details = this.engine().status();
  Ok(cx.string(details.to_string()))
}

pub fn toBuffer(mut cx: FunctionContext) -> JsResult<JsPromise> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let pages = pages_arg(&mut cx, 1, &this)?;
  let file_format = string_arg(&mut cx, 2, "format")?;
  let quality = float_arg(&mut cx, 3, "quality")?;
  let density = float_arg(&mut cx, 4, "density")?;
  let outline = bool_arg(&mut cx, 5, "outline")?;
  let matte = color_arg(&mut cx, 6);

  let channel = cx.channel();
  let (deferred, promise) = cx.promise();
  rayon::spawn(move || {
    let result = {
      if file_format=="pdf" && pages.len() > 1 {
        pages.as_pdf(quality, density, matte)
      }else{
        pages.first().encoded_as(&file_format, quality, density, outline, matte, pages.engine)
      }
    };

    deferred.settle_with(&channel, move |mut cx| {
      let data = result.or_else(|err| cx.throw_error(err))?;
      let mut buffer = cx.buffer(data.len())?;
      buffer.as_mut_slice(&mut cx).copy_from_slice(&data);
      Ok(buffer)
    });
  });

  Ok(promise)
}

pub fn toBufferSync(mut cx: FunctionContext) -> JsResult<JsValue> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let pages = pages_arg(&mut cx, 1, &this)?;
  let file_format = string_arg(&mut cx, 2, "format")?;
  let quality = float_arg(&mut cx, 3, "quality")?;
  let density = float_arg(&mut cx, 4, "density")?;
  let outline = bool_arg(&mut cx, 5, "outline")?;
  let matte = color_arg(&mut cx, 6);

    let encoded = {
      if file_format=="pdf" && pages.len() > 1 {
        pages.as_pdf(quality, density, matte)
      }else{
        pages.first().encoded_as(&file_format, quality, density, outline, matte, pages.engine)
      }
    };

    match encoded{
      Ok(data) => {
        let mut buffer = cx.buffer(data.len())?;
        buffer.as_mut_slice(&mut cx).copy_from_slice(&data);
        Ok(buffer.upcast::<JsValue>())
      },
      Err(msg) => cx.throw_error(msg)
    }
}

pub fn save(mut cx: FunctionContext) -> JsResult<JsPromise> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let pages = pages_arg(&mut cx, 1, &this)?;
  let name_pattern = string_arg(&mut cx, 2, "filePath")?;
  let sequence = !cx.argument::<JsValue>(3)?.is_a::<JsUndefined, _>(&mut cx);
  let padding = opt_float_arg(&mut cx, 3).unwrap_or(-1.0);
  let file_format = string_arg(&mut cx, 4, "format")?;
  let quality = float_arg(&mut cx, 5, "quality")?;
  let density = float_arg(&mut cx, 6, "density")?;
  let outline = bool_arg(&mut cx, 7, "outline")?;
  let matte = color_arg(&mut cx, 8);

  let channel = cx.channel();
  let (deferred, promise) = cx.promise();
  rayon::spawn(move || {
    let result = {
      if sequence {
        pages.write_sequence(&name_pattern, &file_format, padding, quality, density, outline, matte)
      } else if file_format == "pdf" {
        pages.write_pdf(&name_pattern, quality, density, matte)
      } else {
        pages.write_image(&name_pattern, &file_format, quality, density, outline, matte)
      }
    };

    deferred.settle_with(&channel, move |mut cx| match result{
      Err(msg) => cx.throw_error(format!("I/O Error: {}", msg)),
      _ => Ok(cx.undefined())
    });
  });

  Ok(promise)
}

pub fn saveSync(mut cx: FunctionContext) -> JsResult<JsUndefined> {
  let this = cx.argument::<BoxedCanvas>(0)?;
  let pages = pages_arg(&mut cx, 1, &this)?;
  let name_pattern = string_arg(&mut cx, 2, "filePath")?;
  let sequence = !cx.argument::<JsValue>(3)?.is_a::<JsUndefined, _>(&mut cx);
  let padding = opt_float_arg(&mut cx, 3).unwrap_or(-1.0);
  let file_format = string_arg(&mut cx, 4, "format")?;
  let quality = float_arg(&mut cx, 5, "quality")?;
  let density = float_arg(&mut cx, 6, "density")?;
  let outline = bool_arg(&mut cx, 7, "outline")?;
  let matte = color_arg(&mut cx, 8);

  let result = {
    if sequence {
      pages.write_sequence(&name_pattern, &file_format, padding, quality, density, outline, matte)
    } else if file_format == "pdf" {
      pages.write_pdf(&name_pattern, quality, density, matte)
    } else {
      pages.write_image(&name_pattern, &file_format, quality, density, outline, matte)
    }
  };

  match result{
    Ok(_) => Ok(cx.undefined()),
    Err(msg) => cx.throw_error(msg)
  }
}
