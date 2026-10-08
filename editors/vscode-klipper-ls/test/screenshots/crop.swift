// Crops a PNG in place: drops `top` pixels, then keeps at most `keep` rows
// and `width` columns.
//   swift crop.swift file.png top keep width
import CoreGraphics
import Foundation
import ImageIO

let args = CommandLine.arguments
guard args.count == 5, let top = Int(args[2]), let keep = Int(args[3]), let width = Int(args[4]),
      let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: args[1]) as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
      let cropped = image.cropping(to: CGRect(x: 0, y: top, width: min(width, image.width), height: min(keep, image.height - top))),
      let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: args[1]) as CFURL, "public.png" as CFString, 1, nil)
else { FileHandle.standardError.write(Data("usage: crop.swift file.png top keep width\n".utf8)); exit(1) }
CGImageDestinationAddImage(dest, cropped, nil)
exit(CGImageDestinationFinalize(dest) ? 0 : 1)
