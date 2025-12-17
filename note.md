# ドキュメント

プロジェクトの設計方針についてはAGENT.mdとdocs/を参照し、必要であれば実際のコードも読む。また、技術的な情報はContext7とWebから集めてください。
このプロジェクトは開発途中で低レイテンシを重視しています。

## パフォーマンスヒント

// #![windows_subsystem = "windows"] // ← これでコンソール非表示（GUIサブシステム）

dda.rs 237行目のコメントアウトを外すと、VSync待機が有効になります。
```rust
```

ビルド時にPATHに`\third_party\llvm\bin`を追加する必要があります。
実行時にPATHに`\third_party\opencv\build\x64\vc16\bin`を追加する必要があります。

レイテンシ最優先なら “YOLO11n（不十分なら s）+ TensorRT FP16” が第一候補。

# 以下確認
findNearestOffsetからエントリー
DetectionMethodはMomentを使用

```cpp
#include "ImageProcessor.h"

ImageProcessor::ImageProcessor(const cv::Scalar& lower, const cv::Scalar& upper, DetectionMethod method)
    : lower_color(lower), upper_color(upper), method(method) {
}

void ImageProcessor::setMethod(DetectionMethod m) { method = m; }
DetectionMethod ImageProcessor::getMethod() const { return method; }

std::vector<cv::Point2f> ImageProcessor::process(const cv::Mat& image) {
    cv::Mat mask = createMask(image);
    switch (method) {
    case DetectionMethod::Contour:
        return detectByContour(mask);
    case DetectionMethod::Moments:
        return detectByMoments(mask);
    default:
        return {};
    }
}

cv::Mat ImageProcessor::createMask(const cv::Mat& image) {
    cv::Mat hsv, mask;
    //cv::cvtColor(image, hsv, cv::COLOR_BGRA2BGR);

    cv::Mat bgr(image.size(), CV_8UC3);
    int fromTo[] = { 0,0, 1,1, 2,2 }; // B,G,R を同じ順でコピー（アルファは無視）
    cv::mixChannels(&image, 1, &bgr, 1, fromTo, 3);

    cv::cvtColor(bgr, hsv, cv::COLOR_BGR2HSV);


    cv::inRange(hsv, lower_color, upper_color, mask);
    return mask;
}

std::vector<cv::Point2f> ImageProcessor::detectByContour(const cv::Mat& mask) {
    std::vector<std::vector<cv::Point>> contours;
    cv::findContours(mask, contours, cv::RETR_EXTERNAL, cv::CHAIN_APPROX_SIMPLE);

    std::vector<cv::Point2f> centers;
    for (const auto& c : contours) {
        double area = cv::contourArea(c);
        if (area < 50) continue;
        cv::Moments m = cv::moments(c);
        if (m.m00 != 0) {
            centers.emplace_back(static_cast<float>(m.m10 / m.m00),
                static_cast<float>(m.m01 / m.m00));
        }
    }
    return centers;
}

std::vector<cv::Point2f> ImageProcessor::detectByMoments(const cv::Mat& mask) {
    std::vector<cv::Point2f> centers;
    cv::Moments m = cv::moments(mask, true);
    if (m.m00 != 0) {
        centers.emplace_back(static_cast<float>(m.m10 / m.m00),
            static_cast<float>(m.m01 / m.m00));
    }
    return centers;
}

bool ImageProcessor::findNearestOffset(const cv::Mat& image,
    cv::Point2f& offset,
    cv::Point2f& nearest,
    bool debug)
{
    auto centers = process(image);
    cv::Point2f imgCenter(image.cols / 2.0f, image.rows / 2.0f);

    // centers が空の場合の処理
    if (centers.empty()) {
        if (debug) {
            cv::Mat debugImg = image.clone();
            // 近傍点が無いので circle は描かず中心マーカーのみ
            cv::drawMarker(debugImg, imgCenter, cv::Scalar(255, 0, 0),
                cv::MARKER_CROSS, 20, 2);
            cv::imshow("Debug Nearest Target", debugImg);
            cv::waitKey(1);
        }
        return false;
    }

    // 最近傍点を探索
    float bestD2 = std::numeric_limits<float>::max();
    cv::Point2f bestPt;
    for (auto& p : centers) {
        float dx = p.x - imgCenter.x;
        float dy = p.y - imgCenter.y;
        float d2 = dx * dx + dy * dy;
        if (d2 < bestD2) {
            bestD2 = d2;
            bestPt = p;
        }
    }

    nearest = bestPt;
    offset = cv::Point2f(bestPt.x - imgCenter.x, bestPt.y - imgCenter.y);

    if (debug) {
        cv::Mat debugImg = image.clone();
        cv::circle(debugImg, nearest, 4, cv::Scalar(0, 255, 0), -1);
        cv::drawMarker(debugImg, imgCenter, cv::Scalar(255, 0, 0),
            cv::MARKER_CROSS, 20, 2);
        cv::imshow("Debug Nearest Target", debugImg);
        cv::waitKey(1);
    }

    return true;
}


void ImageProcessor::debugNoiseRemoval(const cv::Mat& image) {
    cv::Mat mask = createMask(image);
    cv::Mat kernel = cv::getStructuringElement(cv::MORPH_ELLIPSE, cv::Size(5, 5));
    cv::Mat open, close, openClose, gauss, median, dilated;

    cv::imshow("Mask Original", mask);

    /*cv::morphologyEx(mask, open, cv::MORPH_OPEN, kernel);
    cv::imshow("Morph Open", open);

    cv::morphologyEx(mask, close, cv::MORPH_CLOSE, kernel);
    cv::imshow("Morph Close", close);

    cv::morphologyEx(mask, openClose, cv::MORPH_OPEN, kernel);
    cv::morphologyEx(openClose, openClose, cv::MORPH_CLOSE, kernel);
    cv::imshow("Morph Open+Close", openClose);*/


    cv::dilate(mask, dilated, cv::Mat(), cv::Point(-1, -1), 3);
    cv::imshow("dilated", dilated);

    /*cv::GaussianBlur(mask, gauss, cv::Size(5, 5), 0);
    cv::imshow("GaussianBlur", gauss);*/

    cv::medianBlur(mask, median, 5);
    cv::imshow("MedianBlur", median);


    cv::waitKey(1);
}

```