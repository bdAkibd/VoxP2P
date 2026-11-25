package com.voxp2p.app

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Looper
import android.os.Message
import android.view.View
import android.view.ViewGroup
import android.webkit.ConsoleMessage
import android.webkit.GeolocationPermissions
import android.webkit.JsPromptResult
import android.webkit.JsResult
import android.webkit.PermissionRequest
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebView
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class MainActivity : TauriActivity() {
    companion object {
        private const val REQ_STARTUP_PERMISSIONS = 1001
        private const val REQ_AUDIO_PERMISSION = 1002
        private const val REQ_CAMERA_PERMISSION = 1003
        private const val PERMISSION_WAIT_TIMEOUT_SECS = 60L
    }

    private var multicastLock: WifiManager.MulticastLock? = null
    private val permissionLock = Any()
    private var audioPermissionLatch: CountDownLatch? = null
    private var cameraPermissionLatch: CountDownLatch? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        try {
            setTheme(androidx.appcompat.R.style.Theme_AppCompat_DayNight_NoActionBar)
        } catch (e: Exception) {
            e.printStackTrace()
        }
        super.onCreate(savedInstanceState)

        requestStartupPermissions()
        window?.decorView?.post { setupWebViewChromeClient() }
    }

    override fun onResume() {
        super.onResume()
        acquireMulticastLock()
        window?.decorView?.post { setupWebViewChromeClient() }
    }

    override fun onPause() {
        releaseMulticastLock()
        super.onPause()
    }

    fun startCallService(title: String, text: String) {
        CallService.startService(this, title, text)
    }

    fun stopCallService() {
        CallService.stopService(this)
    }

    fun hasRecordAudioPermission(): Boolean {
        return ContextCompat.checkSelfPermission(
            this,
            Manifest.permission.RECORD_AUDIO
        ) == PackageManager.PERMISSION_GRANTED
    }

    fun hasCameraPermission(): Boolean {
        return ContextCompat.checkSelfPermission(
            this,
            Manifest.permission.CAMERA
        ) == PackageManager.PERMISSION_GRANTED
    }

    fun ensureAudioPermissions(): Boolean {
        if (hasRecordAudioPermission()) {
            return true
        }
        return requestSinglePermissionAndWait(
            permission = Manifest.permission.RECORD_AUDIO,
            requestCode = REQ_AUDIO_PERMISSION,
            isAudio = true
        )
    }

    fun ensureCameraPermissions(): Boolean {
        if (hasCameraPermission()) {
            return true
        }
        return requestSinglePermissionAndWait(
            permission = Manifest.permission.CAMERA,
            requestCode = REQ_CAMERA_PERMISSION,
            isAudio = false
        )
    }

    private fun requestSinglePermissionAndWait(
        permission: String,
        requestCode: Int,
        isAudio: Boolean
    ): Boolean {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            ActivityCompat.requestPermissions(this, arrayOf(permission), requestCode)
            return false
        }

        val latch = CountDownLatch(1)
        synchronized(permissionLock) {
            if (isAudio) {
                audioPermissionLatch?.countDown()
                audioPermissionLatch = latch
            } else {
                cameraPermissionLatch?.countDown()
                cameraPermissionLatch = latch
            }
        }

        runOnUiThread {
            try {
                ActivityCompat.requestPermissions(this, arrayOf(permission), requestCode)
            } catch (e: Exception) {
                e.printStackTrace()
                latch.countDown()
            }
        }

        try {
            latch.await(PERMISSION_WAIT_TIMEOUT_SECS, TimeUnit.SECONDS)
        } catch (_: InterruptedException) {
            Thread.currentThread().interrupt()
        }

        return if (isAudio) hasRecordAudioPermission() else hasCameraPermission()
    }

    private fun requestStartupPermissions() {
        val missing = mutableListOf<String>()
        if (!hasRecordAudioPermission()) {
            missing.add(Manifest.permission.RECORD_AUDIO)
        }
        if (!hasCameraPermission()) {
            missing.add(Manifest.permission.CAMERA)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val hasNotif = ContextCompat.checkSelfPermission(
                this,
                Manifest.permission.POST_NOTIFICATIONS
            ) == PackageManager.PERMISSION_GRANTED
            if (!hasNotif) {
                missing.add(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
        if (missing.isNotEmpty()) {
            try {
                ActivityCompat.requestPermissions(
                    this,
                    missing.toTypedArray(),
                    REQ_STARTUP_PERMISSIONS
                )
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        synchronized(permissionLock) {
            if (requestCode == REQ_AUDIO_PERMISSION || requestCode == REQ_STARTUP_PERMISSIONS) {
                audioPermissionLatch?.countDown()
                audioPermissionLatch = null
            }
            if (requestCode == REQ_CAMERA_PERMISSION || requestCode == REQ_STARTUP_PERMISSIONS) {
                cameraPermissionLatch?.countDown()
                cameraPermissionLatch = null
            }
        }
    }

    private fun acquireMulticastLock() {
        try {
            if (multicastLock == null) {
                val wifiManager =
                    applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
                multicastLock = wifiManager?.createMulticastLock("VoxP2P_mDNS")?.apply {
                    setReferenceCounted(false)
                }
            }
            if (multicastLock?.isHeld == false) {
                multicastLock?.acquire()
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
    }

    private fun releaseMulticastLock() {
        try {
            if (multicastLock?.isHeld == true) {
                multicastLock?.release()
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
    }

    private fun setupWebViewChromeClient() {
        try {
            val root = window?.decorView ?: return
            val webView = findWebView(root) ?: return
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val existing = webView.webChromeClient
                if (existing !is VoxWebChromeClient) {
                    webView.webChromeClient = VoxWebChromeClient(existing)
                }
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
    }

    private fun findWebView(view: View): WebView? {
        if (view is WebView) return view
        if (view is ViewGroup) {
            for (i in 0 until view.childCount) {
                val found = findWebView(view.getChildAt(i))
                if (found != null) return found
            }
        }
        return null
    }

    private inner class VoxWebChromeClient(
        private val delegate: WebChromeClient?
    ) : WebChromeClient() {

        override fun onPermissionRequest(request: PermissionRequest?) {
            val req = request ?: return
            val resources = req.resources ?: emptyArray()
            val granted = mutableListOf<String>()
            var hasMediaResource = false

            for (res in resources) {
                when (res) {
                    PermissionRequest.RESOURCE_AUDIO_CAPTURE -> {
                        hasMediaResource = true
                        if (hasRecordAudioPermission()) {
                            granted.add(res)
                        }
                    }
                    PermissionRequest.RESOURCE_VIDEO_CAPTURE -> {
                        hasMediaResource = true
                        if (hasCameraPermission()) {
                            granted.add(res)
                        }
                    }
                }
            }

            if (granted.isNotEmpty()) {
                req.grant(granted.toTypedArray())
            } else if (hasMediaResource) {
                req.deny()
            } else if (delegate != null) {
                delegate.onPermissionRequest(req)
            } else {
                req.deny()
            }
        }

        override fun onPermissionRequestCanceled(request: PermissionRequest?) {
            delegate?.onPermissionRequestCanceled(request)
                ?: super.onPermissionRequestCanceled(request)
        }

        override fun onShowFileChooser(
            webView: WebView?,
            filePathCallback: ValueCallback<Array<Uri>>?,
            fileChooserParams: FileChooserParams?
        ): Boolean {
            return delegate?.onShowFileChooser(webView, filePathCallback, fileChooserParams)
                ?: super.onShowFileChooser(webView, filePathCallback, fileChooserParams)
        }

        override fun onConsoleMessage(consoleMessage: ConsoleMessage?): Boolean {
            return delegate?.onConsoleMessage(consoleMessage)
                ?: super.onConsoleMessage(consoleMessage)
        }

        override fun onJsAlert(
            view: WebView?,
            url: String?,
            message: String?,
            result: JsResult?
        ): Boolean {
            return delegate?.onJsAlert(view, url, message, result)
                ?: super.onJsAlert(view, url, message, result)
        }

        override fun onJsConfirm(
            view: WebView?,
            url: String?,
            message: String?,
            result: JsResult?
        ): Boolean {
            return delegate?.onJsConfirm(view, url, message, result)
                ?: super.onJsConfirm(view, url, message, result)
        }

        override fun onJsPrompt(
            view: WebView?,
            url: String?,
            message: String?,
            defaultValue: String?,
            result: JsPromptResult?
        ): Boolean {
            return delegate?.onJsPrompt(view, url, message, defaultValue, result)
                ?: super.onJsPrompt(view, url, message, defaultValue, result)
        }

        override fun onProgressChanged(view: WebView?, newProgress: Int) {
            delegate?.onProgressChanged(view, newProgress)
                ?: super.onProgressChanged(view, newProgress)
        }

        override fun onReceivedTitle(view: WebView?, title: String?) {
            delegate?.onReceivedTitle(view, title)
                ?: super.onReceivedTitle(view, title)
        }

        override fun onShowCustomView(view: View?, callback: CustomViewCallback?) {
            delegate?.onShowCustomView(view, callback)
                ?: super.onShowCustomView(view, callback)
        }

        override fun onHideCustomView() {
            delegate?.onHideCustomView() ?: super.onHideCustomView()
        }

        override fun onCreateWindow(
            view: WebView?,
            isDialog: Boolean,
            isUserGesture: Boolean,
            resultMsg: Message?
        ): Boolean {
            return delegate?.onCreateWindow(view, isDialog, isUserGesture, resultMsg)
                ?: super.onCreateWindow(view, isDialog, isUserGesture, resultMsg)
        }

        override fun onCloseWindow(window: WebView?) {
            delegate?.onCloseWindow(window) ?: super.onCloseWindow(window)
        }

        override fun onGeolocationPermissionsShowPrompt(
            origin: String?,
            callback: GeolocationPermissions.Callback?
        ) {
            delegate?.onGeolocationPermissionsShowPrompt(origin, callback)
                ?: super.onGeolocationPermissionsShowPrompt(origin, callback)
        }

        override fun onGeolocationPermissionsHidePrompt() {
            delegate?.onGeolocationPermissionsHidePrompt()
                ?: super.onGeolocationPermissionsHidePrompt()
        }

        override fun getDefaultVideoPoster(): Bitmap? {
            return delegate?.defaultVideoPoster ?: super.getDefaultVideoPoster()
        }

        override fun getVideoLoadingProgressView(): View? {
            return delegate?.videoLoadingProgressView ?: super.getVideoLoadingProgressView()
        }
    }
}
