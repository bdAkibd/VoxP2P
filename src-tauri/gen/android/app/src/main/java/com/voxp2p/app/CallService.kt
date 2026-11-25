package com.voxp2p.app

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat

class CallService : Service() {
    companion object {
        private const val TAG = "VoxP2P"
        const val ACTION_START = "com.voxp2p.app.action.START_CALL"
        const val ACTION_STOP = "com.voxp2p.app.action.STOP_CALL"
        const val CHANNEL_ID = "voxp2p_active_call_channel"
        const val NOTIFICATION_ID = 4040

        private var wakeLock: PowerManager.WakeLock? = null

        @JvmStatic
        fun startService(context: Context, title: String, text: String) {
            try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    val hasMicPermission = ContextCompat.checkSelfPermission(
                        context,
                        Manifest.permission.RECORD_AUDIO
                    ) == PackageManager.PERMISSION_GRANTED
                    if (!hasMicPermission) {
                        Log.w(TAG, "[VoxP2P] CallService.startService skipped: RECORD_AUDIO not granted")
                        return
                    }
                }
                Log.i(TAG, "[VoxP2P] CallService.startService(title=$title)")
                val intent = Intent(context, CallService::class.java).apply {
                    action = ACTION_START
                    putExtra("title", title)
                    putExtra("text", text)
                }
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                    context.startForegroundService(intent)
                } else {
                    context.startService(intent)
                }
            } catch (e: Exception) {
                Log.e(TAG, "[VoxP2P] CallService.startService failed", e)
            }
        }

        @JvmStatic
        fun stopService(context: Context) {
            try {
                Log.i(TAG, "[VoxP2P] CallService.stopService")
                val intent = Intent(context, CallService::class.java)
                context.stopService(intent)
            } catch (e: Exception) {
                Log.e(TAG, "[VoxP2P] CallService.stopService failed", e)
            }
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            Log.i(TAG, "[VoxP2P] CallService.onStartCommand: ACTION_STOP")
            releaseWakeLock()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                stopForeground(STOP_FOREGROUND_REMOVE)
            } else {
                @Suppress("DEPRECATION")
                stopForeground(true)
            }
            stopSelf()
            return START_NOT_STICKY
        }

        val title = intent?.getStringExtra("title") ?: "VoxP2P"
        val text = intent?.getStringExtra("text") ?: "Активный защищенный звонок"
        Log.i(TAG, "[VoxP2P] CallService.onStartCommand: starting foreground (title=$title)")

        createNotificationChannel()
        val notification = buildNotification(title, text)

        acquireWakeLock()

        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (e: SecurityException) {
            Log.e(TAG, "[VoxP2P] CallService.startForeground SecurityException", e)
            releaseWakeLock()
            stopSelf()
            return START_NOT_STICKY
        } catch (e: Exception) {
            Log.e(TAG, "[VoxP2P] CallService.startForeground failed", e)
            releaseWakeLock()
            stopSelf()
            return START_NOT_STICKY
        }

        return START_STICKY
    }

    private fun acquireWakeLock() {
        if (wakeLock == null) {
            try {
                val powerManager = getSystemService(Context.POWER_SERVICE) as PowerManager
                wakeLock = powerManager.newWakeLock(
                    PowerManager.PARTIAL_WAKE_LOCK,
                    "VoxP2P::CallWakeLock"
                ).apply {
                    setReferenceCounted(false)
                    acquire(4 * 60 * 60 * 1000L) // 4 hours maximum timeout
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
    }

    private fun releaseWakeLock() {
        try {
            wakeLock?.let {
                if (it.isHeld) {
                    it.release()
                }
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
        wakeLock = null
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = getSystemService(NotificationManager::class.java)
            val channel = NotificationChannel(
                CHANNEL_ID,
                "VoxP2P Active Call",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Shows persistent status during active VoxP2P voice calls"
                setShowBadge(false)
            }
            manager?.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(title: String, text: String): Notification {
        val launchIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val pendingIntent = PendingIntent.getActivity(
            this,
            0,
            launchIntent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle(title)
            .setContentText(text)
            .setSmallIcon(android.R.drawable.stat_sys_phone_call)
            .setOngoing(true)
            .setContentIntent(pendingIntent)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .build()
    }

    override fun onDestroy() {
        Log.i(TAG, "[VoxP2P] CallService.onDestroy")
        releaseWakeLock()
        super.onDestroy()
    }
}
