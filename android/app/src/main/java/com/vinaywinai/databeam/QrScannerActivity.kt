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
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.*
import androidx.camera.core.resolutionselector.*
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import com.google.mlkit.vision.barcode.*
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Fresh scanner: CameraX owns camera/lifecycle/rotation, ML Kit owns QR detection. */
class QrScannerActivity:ComponentActivity() {
    companion object {
        const val RESULT_CODE="sharing-code"
        fun scannerOptions()=BarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
    }
    private lateinit var preview:PreviewView
    private lateinit var status:TextView
    private lateinit var torch:Button
    private var provider:ProcessCameraProvider?=null
    private var camera:Camera?=null
    private var scanner:BarcodeScanner?=null
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
            if(android.os.Build.VERSION.SDK_INT>=30){val bars=insets.getInsets(WindowInsets.Type.systemBars());view.setPadding(bars.left,bars.top,bars.right,bars.bottom)}
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
                val maxZoom=camera!!.cameraInfo.zoomState.value?.maxZoomRatio?.coerceAtMost(4f) ?: 2f
                scanner=BarcodeScanning.getClient(BarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE)
                    .setZoomSuggestionOptions(ZoomSuggestionOptions.Builder{ratio->
                        val current=camera
                        if(current==null || finished.get())false else {
                            val maximum=current.cameraInfo.zoomState.value?.maxZoomRatio ?: maxZoom
                            current.cameraControl.setZoomRatio(ratio.coerceIn(1f,maximum));true
                        }
                    }.setMaxSupportedZoomRatio(maxZoom).build()).build())
                analysis.setAnalyzer(executor){image->analyse(image)}
            }.onFailure{status.text="Could not open the camera: ${it.message}. Go back and try scanning again."}
        },ContextCompat.getMainExecutor(this))
    }
    @androidx.annotation.OptIn(markerClass=[ExperimentalGetImage::class])
    private fun analyse(image:ImageProxy){
        if(finished.get() || !processing.compareAndSet(false,true)){image.close();return}
        val media=image.image;val reader=scanner
        if(media==null || reader==null){processing.set(false);image.close();return}
        val input=InputImage.fromMediaImage(media,image.imageInfo.rotationDegrees)
        reader.process(input).addOnSuccessListener{barcodes->
            barcodes.firstNotNullOfOrNull{it.rawValue?.takeIf(String::isNotBlank)}?.let{value->
                if(finished.compareAndSet(false,true)){
                    setResult(RESULT_OK,Intent().putExtra(RESULT_CODE,value));finish()
                }
            }
        }.addOnFailureListener{if(!isDestroyed)status.text="Could not read this frame. Keep the whole QR visible and tap to focus."}
            .addOnCompleteListener{image.close();processing.set(false)}
    }
    override fun onDestroy(){
        finished.set(true);provider?.unbindAll();scanner?.close();executor.shutdown();super.onDestroy()
    }
}
