package com.vinaywinai.databeam

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Bundle
import android.util.Size
import android.view.*
import android.widget.*
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.*
import androidx.camera.core.resolutionselector.*
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import com.google.zxing.*
import com.google.zxing.common.GlobalHistogramBinarizer
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

/** On-device scanner: CameraX owns camera/lifecycle/rotation, ZXing decodes QR frames locally. */
class QrScannerActivity:ComponentActivity() {
    companion object {
        const val RESULT_CODE="sharing-code"
        private val decodeHints=mapOf(
            DecodeHintType.TRY_HARDER to true,
            DecodeHintType.CHARACTER_SET to "UTF-8"
        )

        private fun tryDecode(reader:QRCodeReader,source:LuminanceSource,global:Boolean=false):String? {
            return try {
                val binarizer=if(global)GlobalHistogramBinarizer(source) else HybridBinarizer(source)
                reader.decode(BinaryBitmap(binarizer),decodeHints).text?.takeIf(String::isNotBlank)
            } catch(_:ReaderException) {
                null
            } finally {
                reader.reset()
            }
        }

        fun decodeLuminance(source:LuminanceSource,frameIndex:Int=0):String? {
            val reader=QRCodeReader()
            tryDecode(reader,source,global=false)?.let{return it}
            if(frameIndex%2==0) {
                val side=(minOf(source.width,source.height)*7)/10
                if(side>=64 && source.isCropSupported) {
                    val cropped=source.crop((source.width-side)/2,(source.height-side)/2,side,side)
                    tryDecode(reader,cropped,global=false)?.let{return it}
                }
            } else {
                tryDecode(reader,source,global=true)?.let{return it}
            }
            if(frameIndex%4==3) {
                tryDecode(reader,source.invert(),global=false)?.let{return it}
            }
            return null
        }

        fun decodeYuv(luma:ByteArray,width:Int,height:Int,frameIndex:Int=0):String? {
            if(width<=0 || height<=0 || luma.size<width*height)return null
            return decodeLuminance(PlanarYUVLuminanceSource(luma,width,height,0,0,width,height,false),frameIndex)
        }

        fun decodePixels(pixels:IntArray,width:Int,height:Int,frameIndex:Int=0):String? {
            if(width<=0 || height<=0 || pixels.size<width*height)return null
            return decodeLuminance(RGBLuminanceSource(width,height,pixels),frameIndex)
        }
    }

    private lateinit var preview:PreviewView
    private lateinit var status:TextView
    private lateinit var torch:Button
    private var provider:ProcessCameraProvider?=null
    private var camera:Camera?=null
    private var lumaScratch=ByteArray(0)
    private val frameCounter=AtomicInteger(0)
    private val executor=Executors.newSingleThreadExecutor()
    private val processing=AtomicBoolean(false)
    private val finished=AtomicBoolean(false)
    private var lit=false
    private val permission=registerForActivityResult(ActivityResultContracts.RequestPermission()){allowed->
        if(allowed)startCamera() else status.text="Camera access is needed to scan. Allow it in Android app settings, or go back to paste a code."
    }
    private fun dp(value:Int)=(resources.displayMetrics.density*value).toInt()
    override fun onCreate(savedInstanceState:Bundle?){
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle=SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle=SystemBarStyle.dark(Color.TRANSPARENT)
        )
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val root=FrameLayout(this).apply{setBackgroundColor(Color.BLACK)}
        preview=PreviewView(this).apply{implementationMode=PreviewView.ImplementationMode.COMPATIBLE;scaleType=PreviewView.ScaleType.FIT_CENTER}
        root.addView(preview,FrameLayout.LayoutParams(-1,-1))
        val header=LinearLayout(this).apply{orientation=LinearLayout.HORIZONTAL;setPadding(dp(12),dp(8),dp(12),dp(8));setBackgroundColor(0xB3000000.toInt())}
        fun button(parent:LinearLayout,value:String,action:()->Unit)=Button(this).apply{text=value;isAllCaps=false;parent.addView(this);setOnClickListener{action()}}
        button(header,"Back"){finish()}
        header.addView(TextView(this).apply{text="Scan sharing QR";setTextColor(Color.WHITE);textSize=18f;gravity=Gravity.CENTER},LinearLayout.LayoutParams(0,-1,1f))
        torch=button(header,"Light"){
            camera?.let{if(it.cameraInfo.hasFlashUnit()){lit=!lit;it.cameraControl.enableTorch(lit);torch.text=if(lit)"Light off" else "Light"}}
        }
        root.addView(header,FrameLayout.LayoutParams(-1,dp(64),Gravity.TOP))
        val footer=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setPadding(dp(18),dp(12),dp(18),dp(14));setBackgroundColor(0xB3000000.toInt())}
        status=TextView(this).apply{text="Point at the sender’s QR. Tap to focus; pinch to zoom.";setTextColor(Color.WHITE);textSize=14f;gravity=Gravity.CENTER;footer.addView(this)}
        val zoom=SeekBar(this).apply{max=100;contentDescription="Camera zoom";footer.addView(this)}
        zoom.setOnSeekBarChangeListener(object:SeekBar.OnSeekBarChangeListener{
            override fun onProgressChanged(view:SeekBar?,progress:Int,fromUser:Boolean){if(fromUser)camera?.cameraControl?.setLinearZoom(progress/100f)}
            override fun onStartTrackingTouch(view:SeekBar?){}
            override fun onStopTrackingTouch(view:SeekBar?){}
        })
        root.addView(footer,FrameLayout.LayoutParams(-1,-2,Gravity.BOTTOM));setContentView(root)
        root.setOnApplyWindowInsetsListener{view,insets->
            if(android.os.Build.VERSION.SDK_INT>=30){val bars=insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout());view.setPadding(bars.left,bars.top,bars.right,bars.bottom)}
            else @Suppress("DEPRECATION") view.setPadding(insets.systemWindowInsetLeft,insets.systemWindowInsetTop,insets.systemWindowInsetRight,insets.systemWindowInsetBottom)
            insets
        }
        val pinch=ScaleGestureDetector(this,object:ScaleGestureDetector.SimpleOnScaleGestureListener(){
            override fun onScale(detector:ScaleGestureDetector):Boolean {
                val current=camera ?: return false;val zoomState=current.cameraInfo.zoomState.value ?: return false
                current.cameraControl.setZoomRatio((zoomState.zoomRatio*detector.scaleFactor).coerceIn(zoomState.minZoomRatio,zoomState.maxZoomRatio));return true
            }
        })
        preview.setOnTouchListener{view,event->
            pinch.onTouchEvent(event)
            if(event.action==MotionEvent.ACTION_UP && !pinch.isInProgress){
                view.performClick();val point=preview.meteringPointFactory.createPoint(event.x,event.y)
                camera?.cameraControl?.startFocusAndMetering(FocusMeteringAction.Builder(point,FocusMeteringAction.FLAG_AF or FocusMeteringAction.FLAG_AE).setAutoCancelDuration(3,TimeUnit.SECONDS).build())
            }
            true
        }
        if(checkSelfPermission(Manifest.permission.CAMERA)==PackageManager.PERMISSION_GRANTED)preview.post{startCamera()} else permission.launch(Manifest.permission.CAMERA)
    }
    private fun startCamera(){
        val future=ProcessCameraProvider.getInstance(this)
        future.addListener({
            if(isDestroyed)return@addListener
            runCatching{
                val owner=future.get();provider=owner
                val resolution=ResolutionSelector.Builder()
                    .setAspectRatioStrategy(AspectRatioStrategy.RATIO_16_9_FALLBACK_AUTO_STRATEGY)
                    .setResolutionStrategy(ResolutionStrategy(Size(1920,1080),ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER)).build()
                val displayRotation=preview.display?.rotation ?: Surface.ROTATION_0
                val live=Preview.Builder().setResolutionSelector(resolution).setTargetRotation(displayRotation).build().also{it.setSurfaceProvider(preview.surfaceProvider)}
                val analysis=ImageAnalysis.Builder().setResolutionSelector(resolution).setTargetRotation(displayRotation).setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
                owner.unbindAll();camera=owner.bindToLifecycle(this,CameraSelector.DEFAULT_BACK_CAMERA,live,analysis)
                torch.isEnabled=camera!!.cameraInfo.hasFlashUnit()
                analysis.setAnalyzer(executor){image->analyse(image)}
            }.onFailure{status.text="Could not open the camera: ${it.message}. Go back and try scanning again."}
        },ContextCompat.getMainExecutor(this))
    }
    private fun analyse(image:ImageProxy){
        if(finished.get() || !processing.compareAndSet(false,true)){image.close();return}
        try {
            val plane=image.planes.firstOrNull() ?: return
            val width=image.width
            val height=image.height
            if(width<=0 || height<=0)return
            val needed=width*height
            if(lumaScratch.size!=needed)lumaScratch=ByteArray(needed)
            val buffer=plane.buffer.duplicate().apply{rewind()}
            val rowStride=plane.rowStride
            val pixelStride=plane.pixelStride
            if(pixelStride==1 && rowStride==width && buffer.remaining()>=needed) {
                buffer.get(lumaScratch,0,needed)
            } else {
                var out=0
                for(row in 0 until height) {
                    val rowStart=row*rowStride
                    if(pixelStride==1) {
                        buffer.position(rowStart)
                        buffer.get(lumaScratch,out,width)
                        out+=width
                    } else {
                        for(col in 0 until width) {
                            lumaScratch[out++]=buffer.get(rowStart+col*pixelStride)
                        }
                    }
                }
            }
            val value=decodeYuv(lumaScratch,width,height,frameCounter.getAndIncrement())
            if(value!=null && finished.compareAndSet(false,true)) {
                runOnUiThread{
                    if(!isDestroyed){setResult(RESULT_OK,Intent().putExtra(RESULT_CODE,value));finish()}
                }
            }
        } catch(_:Throwable) {
            // Skip malformed camera frames and continue scanning on the next frame.
        } finally {
            image.close()
            processing.set(false)
        }
    }
    override fun onDestroy(){
        finished.set(true);provider?.unbindAll();executor.shutdown();super.onDestroy()
    }
}
