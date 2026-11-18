Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
public static class GifInfo {
  public static string Check(string path) {
    using (var img = Image.FromFile(path)) {
      int frames = img.GetFrameCount(FrameDimension.Time);
      return string.Format("{0}x{1} frames={2} raw={3}", img.Width, img.Height, frames, img.RawFormat);
    }
  }
}
'@ -ReferencedAssemblies 'System.Drawing'
Write-Host ([GifInfo]::Check('docs\assets\faast-demo.gif'))
